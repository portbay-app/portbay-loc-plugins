//! `@portbay/swc-plugin-loc` — SWC/Turbopack port of the babel/vite loc stamper.
//!
//! Stamps each JSX opening element with its authored source location, anchored
//! at the opening `<` in the *authored* source file. Exact parity with
//! `@portbay/babel-plugin-loc` / `@portbay/vite-plugin-loc`:
//!
//!   * Host element (lowercase intrinsic `<div>`, `<button>`) —
//!     `data-pb-loc="<relpath>:<line>:<col>"` (renders a DOM node).
//!   * Component call site (uppercase `<Hero title="…" />`) —
//!     `data-pb-comp="<relpath>:<line>:<col>"` (the JSX call site).
//!
//! The two use DISTINCT attribute names on purpose. A component renders no DOM
//! node of its own, so `data-pb-comp` rides as a *prop*: React copies enumerable
//! props into the component fiber's `memoizedProps`, so PortBay's editor reads
//! the call-site coordinate off the resolved fiber at runtime — a genuine SOURCE
//! coordinate needing no runtime sourcemap reversal. PortBay's Rust resolver
//! reads either attribute to map a rendered node back to its precise source
//! span. See the repo README for the contract.
//!
//! Member (`<Foo.Bar>`) and namespaced (`<svg:rect>`) tags are skipped — an
//! explicit v1 gap for member/namespaced components.
//!
//! Dev-only by design: PortBay only wires the plugin into `next dev`. The gate
//! also honours an explicit `enabled` config flag and, when that is unset, the
//! host's `NODE_ENV` (production => off).

use std::path::Path;

use serde::Deserialize;
use swc_core::common::{SourceMapper, Span, DUMMY_SP};
use swc_core::ecma::ast::{
    IdentName, JSXAttr, JSXAttrName, JSXAttrOrSpread, JSXAttrValue, JSXElementName,
    JSXOpeningElement, Str,
};
use swc_core::ecma::visit::{VisitMut, VisitMutWith};

const ATTR_HOST: &str = "data-pb-loc";
const ATTR_COMP: &str = "data-pb-comp";

/// Plugin config, JSON-decoded from the `.swcrc` / `next.config`
/// `experimental.swcPlugins` entry.
#[derive(Debug, Default, Deserialize)]
pub struct Config {
    /// Project root. Only files *under* it are stamped, and the emitted path is
    /// relative to it (POSIX). Defaults to empty => stamp nothing (safe).
    #[serde(default)]
    pub root: Option<String>,
    /// Hard on/off switch. When unset, the host's `NODE_ENV` decides
    /// (`production` => off, anything else => on).
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// Resolve the dev-only gate: explicit `enabled` wins, else `NODE_ENV` (only
/// `"production"` turns it off). Mirrors the babel plugin's `isEnabled`, minus
/// the `PORTBAY_LOC` env escape hatch (the wasm sandbox can't read process env;
/// PortBay passes `enabled` through config instead).
pub fn resolve_enabled(enabled_cfg: Option<bool>, env: Option<&str>) -> bool {
    enabled_cfg.unwrap_or(env != Some("production"))
}

/// Project-root-relative POSIX path, or `None` when `filename` sits outside
/// `root` (we never emit a `..` path the resolver would reject). Mirrors the
/// babel plugin's `relPosix`.
pub fn rel_posix(root: &str, filename: &str) -> Option<String> {
    if filename.is_empty() || root.is_empty() {
        return None;
    }
    let rel = Path::new(filename).strip_prefix(root).ok()?;
    let rel = rel.to_string_lossy().replace('\\', "/");
    if rel.is_empty() {
        return None;
    }
    Some(rel)
}

/// The stamp attribute for a JSX tag, or `None` when the tag is skipped.
///
///   * lowercase plain `JSXIdentifier` (`<div>`, `<my-widget>`) => host DOM
///     node => `data-pb-loc`.
///   * uppercase plain `JSXIdentifier` (`<Hero>`) => Component call site =>
///     `data-pb-comp`.
///   * member (`<Foo.Bar>`) / namespaced (`<svg:rect>`) => `None` (v1 gap).
fn stamp_attr(name: &JSXElementName) -> Option<&'static str> {
    match name {
        JSXElementName::Ident(ident) => match ident.sym.as_ref().chars().next() {
            Some(c) if c.is_ascii_lowercase() => Some(ATTR_HOST),
            Some(_) => Some(ATTR_COMP),
            None => None,
        },
        _ => None,
    }
}

/// Idempotency guard: is this element already stamped with `attr`? Host and
/// component names are distinct, so they never collide.
fn has_loc_attr(el: &JSXOpeningElement, attr: &str) -> bool {
    el.attrs.iter().any(|a| {
        matches!(
            a,
            JSXAttrOrSpread::JSXAttr(JSXAttr {
                name: JSXAttrName::Ident(n),
                ..
            }) if n.sym.as_ref() == attr
        )
    })
}

/// The AST transform. Generic over the source map so the native test harness
/// (a real [`swc_core::common::SourceMap`]) and the wasm plugin runtime (a
/// `PluginSourceMapProxy`) drive the exact same code.
pub struct LocVisitor<'a, S: SourceMapper> {
    source_map: &'a S,
    /// Pre-resolved root-relative path; `None` => stamp nothing for this file.
    rel: Option<String>,
    enabled: bool,
}

impl<'a, S: SourceMapper> LocVisitor<'a, S> {
    pub fn new(source_map: &'a S, rel: Option<String>, enabled: bool) -> Self {
        Self {
            source_map,
            rel,
            enabled,
        }
    }

    fn loc_value(&self, span: Span) -> Option<String> {
        let rel = self.rel.as_ref()?;
        let loc = self.source_map.lookup_char_pos(span.lo);
        // Babel emits 1-based line + 1-based column of the opening `<`.
        // SWC's `Loc.line` is already 1-based; `Loc.col` is a 0-based CharPos.
        Some(format!("{rel}:{}:{}", loc.line, loc.col.0 + 1))
    }
}

impl<S: SourceMapper> VisitMut for LocVisitor<'_, S> {
    fn visit_mut_jsx_opening_element(&mut self, el: &mut JSXOpeningElement) {
        // Descend first so JSX nested inside attribute values is stamped too.
        el.visit_mut_children_with(self);

        if !self.enabled || self.rel.is_none() {
            return;
        }
        let Some(attr) = stamp_attr(&el.name) else {
            return;
        };
        if has_loc_attr(el, attr) {
            return;
        }
        let Some(value) = self.loc_value(el.span) else {
            return;
        };

        // Appended *last* so a `{...spread}` can never override the loc — same
        // ordering guarantee the babel plugin makes.
        el.attrs.push(JSXAttrOrSpread::JSXAttr(JSXAttr {
            span: DUMMY_SP,
            name: JSXAttrName::Ident(IdentName::new(attr.into(), DUMMY_SP)),
            value: Some(JSXAttrValue::Str(Str {
                span: DUMMY_SP,
                value: value.into(),
                raw: None,
            })),
        }));
    }
}

// ---------------------------------------------------------------------------
// Plugin entry point (wasm). Tests below exercise `LocVisitor` directly.
// ---------------------------------------------------------------------------

use swc_core::ecma::ast::Program;
use swc_core::plugin::{
    metadata::TransformPluginMetadataContextKind, plugin_transform,
    proxies::TransformPluginProgramMetadata,
};

#[plugin_transform]
fn process_transform(mut program: Program, metadata: TransformPluginProgramMetadata) -> Program {
    let config: Config = metadata
        .get_transform_plugin_config()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();

    let filename = metadata
        .get_context(&TransformPluginMetadataContextKind::Filename)
        .unwrap_or_default();
    let env = metadata.get_context(&TransformPluginMetadataContextKind::Env);

    let enabled = resolve_enabled(config.enabled, env.as_deref());
    let root = config.root.unwrap_or_default();
    let rel = rel_posix(&root, &filename);

    let mut visitor = LocVisitor::new(&metadata.source_map, rel, enabled);
    program.visit_mut_with(&mut visitor);
    program
}

#[cfg(test)]
mod tests {
    use super::*;
    use swc_core::common::sync::Lrc;
    use swc_core::common::{FileName, SourceMap};
    use swc_core::ecma::codegen::{text_writer::JsWriter, Config as CodegenConfig, Emitter};
    use swc_core::ecma::parser::{lexer::Lexer, EsSyntax, Parser, StringInput, Syntax, TsSyntax};

    struct Opts {
        file: &'static str,
        root: &'static str,
        enabled: bool,
        ts: bool,
    }

    impl Default for Opts {
        fn default() -> Self {
            Opts {
                file: "src/App.jsx",
                root: "/proj",
                enabled: true,
                ts: false,
            }
        }
    }

    /// Parse `src` as JSX/TSX, run the visitor, emit code back to a string —
    /// the same shape as the babel package's vitest `run()` helper.
    fn run(src: &str, opts: Opts) -> String {
        let cm: Lrc<SourceMap> = Default::default();
        let abs = format!("{}/{}", opts.root, opts.file);
        let fm = cm.new_source_file(Lrc::new(FileName::Real(abs.clone().into())), src.to_string());

        let syntax = if opts.ts {
            Syntax::Typescript(TsSyntax {
                tsx: true,
                ..Default::default()
            })
        } else {
            Syntax::Es(EsSyntax {
                jsx: true,
                ..Default::default()
            })
        };
        let lexer = Lexer::new(syntax, Default::default(), StringInput::from(&*fm), None);
        let mut parser = Parser::new_from(lexer);
        let mut module = parser.parse_module().expect("parse");

        let rel = rel_posix(opts.root, &abs);
        let mut v = LocVisitor::new(&*cm, rel, opts.enabled);
        module.visit_mut_with(&mut v);

        let mut buf = Vec::new();
        {
            let wr = JsWriter::new(cm.clone(), "\n", &mut buf, None);
            let mut emitter = Emitter {
                cfg: CodegenConfig::default(),
                cm: cm.clone(),
                comments: None,
                wr,
            };
            emitter.emit_module(&module).unwrap();
        }
        String::from_utf8(buf).unwrap()
    }

    /// All values of `attr` in emit order.
    fn all_of(out: &str, attr: &str) -> Vec<String> {
        let needle = format!("{attr}=\"");
        let mut locs = Vec::new();
        let mut rest = out;
        while let Some(i) = rest.find(&needle) {
            rest = &rest[i + needle.len()..];
            if let Some(j) = rest.find('"') {
                locs.push(rest[..j].to_string());
                rest = &rest[j + 1..];
            } else {
                break;
            }
        }
        locs
    }

    /// All `data-pb-loc` (host) values in emit order.
    fn all_locs(out: &str) -> Vec<String> {
        all_of(out, "data-pb-loc")
    }

    /// All `data-pb-comp` (component call-site) values in emit order.
    fn all_comps(out: &str) -> Vec<String> {
        all_of(out, "data-pb-comp")
    }

    /// Every emitted loc must point exactly at a `<` in the ORIGINAL source —
    /// the property PortBay's Rust resolver depends on.
    fn assert_anchor_at_bracket(loc: &str, src: &str) {
        let mut it = loc.rsplitn(3, ':');
        let col: usize = it.next().unwrap().parse().expect("col");
        let line: usize = it.next().unwrap().parse().expect("line");
        let l = src.lines().nth(line - 1).expect("line in source");
        assert_eq!(
            l.as_bytes()[col - 1],
            b'<',
            "loc {loc} must anchor at '<' (line: {l:?})"
        );
    }

    #[test]
    fn stamps_host_and_anchors_col_at_bracket() {
        let code = "export default function App() {\n  return <div className=\"card\">Hi</div>;\n}";
        let out = run(code, Opts::default());
        let locs = all_locs(&out);
        assert_eq!(locs.len(), 1);
        assert!(locs[0].starts_with("src/App.jsx:2:"));
        assert_anchor_at_bracket(&locs[0], code);
        // className survives — this is what unlocks React class write-back.
        assert!(out.contains("className=\"card\""));
    }

    #[test]
    fn stamps_host_with_loc_and_component_with_comp() {
        let code = "const App = () => (\n  <Hero>\n    <span>x</span>\n  </Hero>\n);";
        let out = run(code, Opts::default());
        let locs = all_locs(&out);
        let comps = all_comps(&out);
        assert_eq!(locs.len(), 1, "only <span> is a host");
        assert_eq!(comps.len(), 1, "only <Hero> is a Component");
        // The two attributes never cross: a Component gets no data-pb-loc and a
        // host gets no data-pb-comp.
        assert!(!out.contains("Hero data-pb-loc"));
        assert!(!out.contains("span data-pb-comp"));
        assert_anchor_at_bracket(&locs[0], code);
        assert_anchor_at_bracket(&comps[0], code);
    }

    #[test]
    fn stamps_component_call_site_at_bracket() {
        let code =
            "export default function App() {\n  return <Hero title=\"Welcome\" count={3} disabled />;\n}";
        let out = run(code, Opts::default());
        let comps = all_comps(&out);
        assert_eq!(comps.len(), 1);
        assert!(comps[0].starts_with("src/App.jsx:2:"));
        assert_anchor_at_bracket(&comps[0], code);
        // Authored props survive verbatim — the source the prop classifier reads.
        assert!(out.contains("title=\"Welcome\""));
        assert!(out.contains("count={3}"));
    }

    #[test]
    fn skips_member_and_namespaced_tags() {
        let code = "const A = () => (\n  <Foo.Bar>\n    <ns:widget />\n  </Foo.Bar>\n);";
        let out = run(code, Opts::default());
        assert_eq!(all_locs(&out).len(), 0);
        assert_eq!(all_comps(&out).len(), 0);
    }

    #[test]
    fn component_stamp_is_idempotent() {
        let code = "const A = () => <Hero title=\"x\" />;";
        let once = run(code, Opts::default());
        let twice = run(&once, Opts::default());
        assert_eq!(all_comps(&twice).len(), 1);
    }

    #[test]
    fn stamps_nested_host_independently() {
        let code = "const A = () => <ul><li>a</li><li>b</li></ul>;";
        let out = run(code, Opts::default());
        let locs = all_locs(&out);
        assert_eq!(locs.len(), 3, "ul + 2 li");
        for l in &locs {
            assert_anchor_at_bracket(l, code);
        }
    }

    #[test]
    fn map_element_yields_one_source_loc() {
        let code =
            "const List = ({ items }) => (\n  <ul>{items.map((it) => <li key={it.id}>{it.label}</li>)}</ul>\n);";
        let out = run(code, Opts::default());
        // <ul> + one source <li> => 2 locs; the rendered copies are runtime.
        let locs = all_locs(&out);
        assert_eq!(locs.len(), 2);
        let li_stamps = out.matches("<li").count();
        let li_with_loc = out
            .match_indices("<li")
            .filter(|(i, _)| {
                out[*i..]
                    .split_once('>')
                    .map(|(open, _)| open.contains("data-pb-loc"))
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(li_stamps, 1, "one <li> in source");
        assert_eq!(li_with_loc, 1);
        for l in &locs {
            assert_anchor_at_bracket(l, code);
        }
    }

    #[test]
    fn does_not_mistake_tsx_generics_for_elements() {
        let code = "const C = () => {\n  const v = useState<Record<string, number>>({});\n  return <div>{v}</div>;\n};";
        let out = run(
            code,
            Opts {
                file: "src/C.tsx",
                ts: true,
                ..Default::default()
            },
        );
        let locs = all_locs(&out);
        assert_eq!(locs.len(), 1, "only <div>");
        assert!(locs[0].starts_with("src/C.tsx:"));
        assert!(out.contains("useState"));
    }

    #[test]
    fn is_idempotent() {
        let code = "const A = () => <div>x</div>;";
        let once = run(code, Opts::default());
        let twice = run(&once, Opts::default());
        assert_eq!(all_locs(&twice).len(), 1);
    }

    #[test]
    fn emits_nothing_when_disabled() {
        let code = "const A = () => <div>x</div>;";
        let out = run(
            code,
            Opts {
                enabled: false,
                ..Default::default()
            },
        );
        assert_eq!(all_locs(&out).len(), 0);
    }

    #[test]
    fn skips_files_outside_root() {
        let rel = rel_posix("/proj", "/elsewhere/App.jsx");
        assert!(rel.is_none());

        let cm: Lrc<SourceMap> = Default::default();
        let fm = cm.new_source_file(
            Lrc::new(FileName::Real("/elsewhere/App.jsx".into())),
            "const A = () => <div>x</div>;".to_string(),
        );
        let lexer = Lexer::new(
            Syntax::Es(EsSyntax {
                jsx: true,
                ..Default::default()
            }),
            Default::default(),
            StringInput::from(&*fm),
            None,
        );
        let mut parser = Parser::new_from(lexer);
        let mut module = parser.parse_module().unwrap();
        let mut v = LocVisitor::new(&*cm, rel, true);
        module.visit_mut_with(&mut v);

        let mut buf = Vec::new();
        {
            let wr = JsWriter::new(cm.clone(), "\n", &mut buf, None);
            let mut emitter = Emitter {
                cfg: CodegenConfig::default(),
                cm: cm.clone(),
                comments: None,
                wr,
            };
            emitter.emit_module(&module).unwrap();
        }
        let out = String::from_utf8(buf).unwrap();
        assert_eq!(all_locs(&out).len(), 0);
    }

    #[test]
    fn rel_posix_contract() {
        assert_eq!(
            rel_posix("/proj", "/proj/src/App.jsx").as_deref(),
            Some("src/App.jsx")
        );
        assert_eq!(rel_posix("/proj", "/elsewhere/App.jsx"), None);
        assert_eq!(rel_posix("/proj", "/proj"), None); // empty rel
        assert_eq!(rel_posix("", "/proj/x.jsx"), None);
        assert_eq!(rel_posix("/proj", ""), None);
    }

    #[test]
    fn resolve_enabled_contract() {
        assert!(!resolve_enabled(None, Some("production")));
        assert!(resolve_enabled(None, Some("development")));
        assert!(resolve_enabled(None, None));
        assert!(resolve_enabled(Some(true), Some("production")));
        assert!(!resolve_enabled(Some(false), Some("development")));
    }
}
