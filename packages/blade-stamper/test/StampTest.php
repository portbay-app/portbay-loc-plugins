<?php

declare(strict_types=1);

/**
 * Pure-PHP unit tests for PbLocStamper — no Laravel required (the masking /
 * stamping logic is the safety-critical part). Run: `php test/StampTest.php`.
 * The end-to-end proof against a real Blade compiler lives in the project spike.
 */

require __DIR__ . '/../src/PbLocStamper.php';

use Portbay\BladeStamper\PbLocStamper;

$pass = 0;
$fail = 0;
function check(string $label, bool $cond, string $detail = ''): void
{
    global $pass, $fail;
    if ($cond) {
        $pass++;
        echo "  ok   $label\n";
    } else {
        $fail++;
        echo "  FAIL $label" . ($detail ? "  -- $detail" : '') . "\n";
    }
}

$rel = 'resources/views/x.blade.php';
$stamp = static fn(string $src): string => PbLocStamper::stamp($src, $rel);
/** All data-pb-loc values in a stamped string. */
$locs = static function (string $out): array {
    preg_match_all('/data-pb-loc="([^"]+)"/', $out, $m);
    return $m[1];
};

// --- real HTML tags get stamped with correct 1-based line:col ---
$out = $stamp("<div>\n  <p>hi</p>\n</div>");
check('div stamped 1:1', in_array("$rel:1:1", $locs($out), true));
check('p stamped 2:3', in_array("$rel:2:3", $locs($out), true), implode(',', $locs($out)));
check('insert lands right after tag name', str_contains($out, "<div data-pb-loc=\"$rel:1:1\">"));

// --- Blade echo {{ }} must not be stamped, and its `<` operator survives ---
$out = $stamp("<p>{{ \$a < 3 ? '<b>' : 'x' }}</p>");
check('no loc inside echo', count($locs($out)) === 1, implode(',', $locs($out)));
check('echo body untouched', str_contains($out, "{{ \$a < 3 ? '<b>' : 'x' }}"));
check('the <b> inside the echo string is NOT stamped', !str_contains($out, '<b data-pb-loc'));

// --- raw echo {!! !!} not stamped (only the outer <div> is) ---
$out = $stamp("<div>{!! '<span>raw</span>' !!}</div>");
check('raw echo <span> not stamped', !str_contains($out, '<span data-pb-loc'));
check('raw echo: only outer div stamped', substr_count($out, 'data-pb-loc') === 1);

// --- Blade comment: a fake tag inside must not be stamped ---
$out = $stamp("{{-- <div>fake</div> --}}\n<div>real</div>");
check('comment fake <div> not stamped', substr_count($out, 'data-pb-loc') === 1);
check('the real <div> on line 2 is stamped 2:1', in_array("$rel:2:1", $locs($out), true));

// --- raw PHP block: string-literal tags inside it must not be stamped ---
$php = "<?php \$x = '<em>no</em>'; " . "?>" . "\n<em>yes</em>";
$out = $stamp($php);
check('php string <em> not stamped', !str_contains($out, "'<em data-pb-loc"));
check('real <em> after php stamped 2:1', in_array("$rel:2:1", $locs($out), true));

// --- directive argument containing `<` must not derail the scan ---
$out = $stamp("@if (\$count < count(\$rows))\n  <span>hi</span>\n@endif");
check('span after @if(...) stamped 2:3', in_array("$rel:2:3", $locs($out), true), implode(',', $locs($out)));
check('@if arg preserved verbatim', str_contains($out, "@if (\$count < count(\$rows))"));

// --- @class(...) directive inside a tag: arg preserved, host stamped ---
$out = $stamp("<footer @class(['on' => \$x])>hi</footer>");
check('footer stamped once', substr_count($out, 'data-pb-loc') === 1);
check('footer stamped 1:1', in_array("$rel:1:1", $locs($out), true));
check('@class arg intact', str_contains($out, "@class(['on' => \$x])"));

// --- Blade component <x-...> tags are skipped ---
$out = $stamp("<x-alert type=\"err\">boom</x-alert>\n<div>d</div>");
check('x-alert not stamped', !str_contains($out, '<x-alert data-pb-loc'));
check('sibling div still stamped', in_array("$rel:2:1", $locs($out), true));

// --- <script>/<style> contents skipped; the tags themselves not stamped ---
$out = $stamp("<script>let a = 1 < 2; let b = '<div>'</script>\n<p>x</p>");
check('script tag not stamped', !str_contains($out, '<script data-pb-loc'));
check('code inside script not stamped', substr_count($out, 'data-pb-loc') === 1);
check('p after script stamped 2:1', in_array("$rel:2:1", $locs($out), true));

// --- @foreach loop body: the <li> gets exactly ONE loc (shared by all copies) ---
$out = $stamp("<ul>\n@foreach (\$items as \$i)\n  <li>{{ \$i }}</li>\n@endforeach\n</ul>");
check('li stamped once at its source line 3', count(array_keys($locs($out), "$rel:3:3")) === 1, implode(',', $locs($out)));

// --- idempotency: an already-stamped tag is not double-stamped ---
$once = $stamp('<div class="a">x</div>');
$twice = $stamp($once);
check('re-stamping is a no-op', $once === $twice);

// --- self-closing / void elements ---
$out = $stamp("<img src=\"a.png\" />\n<br>");
check('img self-closing stamped', str_contains($out, "<img data-pb-loc=\"$rel:1:1\""));
check('br void stamped', in_array("$rel:2:1", $locs($out), true));

// --- nothing to stamp returns input unchanged ---
$plain = "just text, {{ \$x }}, no tags";
check('no tags => unchanged', $stamp($plain) === $plain);
check('empty => unchanged', $stamp('') === '');

// --- @@ escaped at-sign is not treated as a directive ---
$out = $stamp("<p>email @@ me</p>\n<div>d</div>");
check('@@ does not swallow following tag', in_array("$rel:2:1", $locs($out), true));

echo "\n=== RESULT: $pass passed, $fail failed ===\n";
exit($fail === 0 ? 0 : 1);
