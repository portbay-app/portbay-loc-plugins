# @portbay/blade-stamper (`portbay/blade-stamper`)

Dev-time **Laravel Blade** source-location stamping for PortBay's visual editor.
It stamps each rendered host element with its authored source location,
`data-pb-loc="<file>:<line>:<col>"`, so a clicked element resolves back to the
exact span in your `.blade.php` file instead of a text-search guess.

```html
<button data-pb-loc="resources/views/home.blade.php:42:7">Get started</button>
```

This is the Blade sibling of [`@portbay/swc-plugin-loc`](../swc-plugin-loc), and
it emits the same `data-pb-loc` shape, so PortBay's resolver handles Blade with
no client changes. With the attribute present you get precise text, class and
attribute editing, plus structural editing. Without it, PortBay falls back to
text search with no change in behaviour.

## How it works (and why it needs no source-map)

Blade compiles server-side, so there is a point where the **raw** template text
is available with its original line numbers. The stamper registers as the
**first** Blade compilation pass via `Blade::prepareStringsForCompilationUsing`,
which runs *before* Blade mutates the string (before comment stripping, `@php`
extraction and `<x-component>` rewriting). Coordinates are read from the pristine
source and baked in as literal attribute strings. Nothing has to be traced back
from compiled output.

A Blade-aware **masking** pass blanks every code region: `{{ }}` / `{!! !!}`
echoes, `{{-- --}}` comments, `<?php … ?>` blocks, `@php … @endphp`,
`@verbatim`, and `@directive(...)` arguments, so a stray `<` inside a PHP
expression is never mistaken for an HTML tag. The mask only guides *where not to
stamp*; inserts are applied to the original text. Over-masking can therefore only
cause a tag to fall back to text-search (safe) and can never corrupt a template.

Blade component tags (`<x-...>`, `<x-slot>`) are intentionally **not** stamped:
their attributes go to the component's attribute bag, not a literal output tag.
Their internals are stamped when their own view is compiled.

## Install

```bash
composer require --dev portbay/blade-stamper
```

Laravel package auto-discovery registers the service provider, so **no config
change needed**. Restart your dev server and clear the compiled views if they
were cached:

```bash
php artisan view:clear
```

## Dev-only, by design

The stamper runs only outside production, checking `app()->environment()`. Leave
it alone and nothing reaches a production render: no extra DOM, no source paths
in shipped HTML.

> **`PORTBAY_LOC=1` overrides that check, and `PORTBAY_LOC=0` disables the
> stamper anywhere.** The override exists so you can debug the stamper in an
> environment that reports itself as production, and it is not safe to leave on.
> Anything you serve with it set publishes your view file names, your directory
> layout and your line numbers to whoever loads the page. Use it on a machine you
> control, then unset it.

## Requirements

- PHP >= 8.1
- Laravel 11 or 12 (`illuminate/view` ^11 || ^12). `prepareStringsForCompilationUsing`
  arrived in Laravel 10.15; on an older compiler the provider no-ops and
  PortBay's text-search resolver stays in effect.

## Test

```bash
composer test   # php test/StampTest.php, pure-PHP unit tests, no Laravel needed
```

## License

MIT © Tribal House LLC. Independent implementation; no third-party plugin code.
