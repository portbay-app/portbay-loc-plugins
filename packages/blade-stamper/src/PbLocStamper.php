<?php

declare(strict_types=1);

namespace Portbay\BladeStamper;

/**
 * Text-based source-location stamper for Laravel Blade templates.
 *
 * It mirrors @portbay/vite-plugin-loc's tolerant markup scanner (Vue/Svelte/
 * Astro/HTML) but adds a Blade-aware MASKING pass so a stray `<` inside a PHP
 * expression, echo, directive argument, `<?php` block or Blade comment is never
 * mistaken for an HTML tag. Each host element gains
 * `data-pb-loc="<relpath>:<line>:<col>"` pointing at the AUTHORED `.blade.php`
 * source — the same coordinate shape the JS plugins emit, so PortBay's existing
 * `data-pb-loc` resolver (loc_resolve.rs) handles Blade with zero changes.
 *
 * WHY THIS NEEDS NO SOURCE-MAP: it runs as the FIRST Blade compilation pass
 * (`prepareStringsForCompilationUsing`), before Blade mutates the string, so
 * line/col are read from the pristine source and baked in as literal attribute
 * strings. Nothing has to be traced back from compiled output.
 *
 * SAFETY POSTURE: the mask is only a "do NOT look for tags here" guide. Inserts
 * are applied to the ORIGINAL text; the mask never appears in output. So
 * over-masking can only cause UNDER-stamping (a tag misses its data-pb-loc and
 * falls back to PortBay's text-search resolver — zero behaviour change) and can
 * NEVER corrupt the template. Corruption would only be possible if we stamped a
 * `<x` that isn't a real output tag; masking the echo/comment/php/directive-arg
 * regions removes exactly those.
 */
final class PbLocStamper
{
    private const ATTR = 'data-pb-loc';

    /** Wrapper tags that produce no DOM node of their own — skipped, children still stamped. */
    private const SKIP = ['script' => true, 'style' => true, 'template' => true, 'slot' => true];

    /**
     * Stamp host elements in a raw Blade template with their source location.
     *
     * @param string $source  raw .blade.php contents
     * @param string $relpath project-root-relative POSIX path (goes verbatim into the attribute)
     * @return string stamped source (byte-for-byte unchanged when there is nothing to stamp)
     */
    public static function stamp(string $source, string $relpath): string
    {
        $n = strlen($source);
        if ($n === 0) {
            return $source;
        }

        // 1. Build a same-length mask blanking every Blade/PHP code region.
        $mask = self::maskCodeRegions($source);

        // 2. Scan the mask for HTML opening tags; collect insert points.
        $inserts = self::scanTags($mask, $source, $relpath);
        if (!$inserts) {
            return $source;
        }

        // 3. Apply inserts to the ORIGINAL, right-to-left so offsets stay valid.
        usort($inserts, static fn($a, $b) => $b[0] <=> $a[0]);
        foreach ($inserts as [$at, $text]) {
            $source = substr($source, 0, $at) . $text . substr($source, $at);
        }

        return $source;
    }

    /** Replace every non-newline byte of each Blade/PHP code region with a space (same length). */
    private static function maskCodeRegions(string $s): string
    {
        $n = strlen($s);
        $mask = $s;

        $blank = static function (int $from, int $to) use (&$mask, $n): void {
            $to = min($to, $n);
            for ($k = $from; $k < $to; $k++) {
                if ($mask[$k] !== "\n") {
                    $mask[$k] = ' ';
                }
            }
        };

        $i = 0;
        while ($i < $n) {
            // Blade comment {{-- ... --}}
            if (self::at($s, $i, '{{--')) {
                $end = strpos($s, '--}}', $i + 4);
                $stop = $end === false ? $n : $end + 4;
                $blank($i, $stop);
                $i = $stop;
                continue;
            }
            // Raw echo {!! ... !!}
            if (self::at($s, $i, '{!!')) {
                $end = strpos($s, '!!}', $i + 3);
                $stop = $end === false ? $n : $end + 3;
                $blank($i, $stop);
                $i = $stop;
                continue;
            }
            // Echo {{ ... }} (also masks the literal-echo @{{ }} harmlessly)
            if (self::at($s, $i, '{{')) {
                $end = strpos($s, '}}', $i + 2);
                $stop = $end === false ? $n : $end + 2;
                $blank($i, $stop);
                $i = $stop;
                continue;
            }
            // Raw PHP open tag: "<?php", "<?=" and short "<?" up to its close.
            if (self::at($s, $i, '<?')) {
                $end = strpos($s, self::PHP_CLOSE, $i + 2);
                $stop = $end === false ? $n : $end + 2;
                $blank($i, $stop);
                $i = $stop;
                continue;
            }
            // @php ... @endphp (block) or @php( ... ) (inline)
            if (self::atDirective($s, $i, 'php')) {
                $after = $i + 4;
                $j = self::skipSpaces($s, $after);
                if ($j < $n && $s[$j] === '(') {
                    $close = self::matchParen($s, $j);
                    $blank($j, $close + 1);
                    $i = $close + 1;
                } else {
                    $end = self::findDirective($s, $after, 'endphp');
                    $stop = $end === false ? $n : $end + 7;
                    $blank($after, $stop);
                    $i = $stop;
                }
                continue;
            }
            // @verbatim ... @endverbatim (masked for safety)
            if (self::atDirective($s, $i, 'verbatim')) {
                $after = $i + 9;
                $end = self::findDirective($s, $after, 'endverbatim');
                $stop = $end === false ? $n : $end + 12;
                $blank($after, $stop);
                $i = $stop;
                continue;
            }
            // @@ escaped literal @ — skip both chars, not a directive.
            if ($s[$i] === '@' && $i + 1 < $n && $s[$i + 1] === '@') {
                $i += 2;
                continue;
            }
            // Generic directive @name( ... ) — mask the balanced-paren argument.
            if ($s[$i] === '@' && $i + 1 < $n && self::isIdentStart($s[$i + 1])) {
                $j = $i + 1;
                while ($j < $n && self::isIdentChar($s[$j])) {
                    $j++;
                }
                $k = self::skipSpaces($s, $j);
                if ($k < $n && $s[$k] === '(') {
                    $close = self::matchParen($s, $k);
                    $blank($k, $close + 1);
                    $i = $close + 1;
                    continue;
                }
                $i = $j;
                continue;
            }
            $i++;
        }

        return $mask;
    }

    /** Tag scan over the MASK; returns [ [offset, insertText], ... ] anchored on the original. */
    private static function scanTags(string $mask, string $orig, string $relpath): array
    {
        $n = strlen($mask);
        $inserts = [];
        $inScript = false;
        $inStyle = false;
        $line = 1;
        $lineStart = 0;
        $i = 0;

        while ($i < $n) {
            $c = $mask[$i];
            if ($c === "\n") {
                $line++;
                $lineStart = $i + 1;
                $i++;
                continue;
            }
            if ($c !== '<') {
                $i++;
                continue;
            }
            // HTML comment
            if (self::at($mask, $i, '<!--')) {
                $end = strpos($mask, '-->', $i + 4);
                $stop = $end === false ? $n : $end + 3;
                while ($i < $stop) {
                    if ($mask[$i] === "\n") {
                        $line++;
                        $lineStart = $i + 1;
                    }
                    $i++;
                }
                continue;
            }
            if (isset($mask[$i + 1]) && ($mask[$i + 1] === '!' || $mask[$i + 1] === '?')) {
                $i++;
                continue;
            }
            $isClose = isset($mask[$i + 1]) && $mask[$i + 1] === '/';
            $j = $i + 1 + ($isClose ? 1 : 0);
            $nameStart = $j;
            while ($j < $n && self::isNameChar($mask[$j])) {
                $j++;
            }
            if ($j === $nameStart) {
                $i++;
                continue;
            }
            $rawName = substr($mask, $nameStart, $j - $nameStart);
            $name = strtolower($rawName);

            if ($isClose) {
                if ($name === 'script') {
                    $inScript = false;
                } elseif ($name === 'style') {
                    $inStyle = false;
                }
                $i = $j;
                continue;
            }

            $ltCol = $i - $lineStart + 1;
            $ltLine = $line;

            // Walk to '>' respecting quoted attribute values (over the mask).
            $k = $j;
            $quote = null;
            while ($k < $n) {
                $ck = $mask[$k];
                if ($quote !== null) {
                    if ($ck === $quote) {
                        $quote = null;
                    }
                } elseif ($ck === '"' || $ck === "'") {
                    $quote = $ck;
                } elseif ($ck === '>') {
                    break;
                }
                $k++;
            }

            $active = !$inScript && !$inStyle;
            $first = $rawName[0];
            $isHost = ($first >= 'a' && $first <= 'z')
                && strpos($rawName, ':') === false
                && !isset(self::SKIP[$name])
                // Skip Blade component tags <x-...> / <x-slot>: their attributes
                // go to the component's attribute bag, not a literal output tag.
                && substr($name, 0, 2) !== 'x-';
            $openSlice = substr($orig, $i, $k - $i);
            $alreadyStamped = strpos($openSlice, self::ATTR) !== false;

            if ($active && $isHost && !$alreadyStamped) {
                $inserts[] = [$j, ' ' . self::ATTR . '="' . $relpath . ':' . $ltLine . ':' . $ltCol . '"'];
            }

            // Entering a wrapper region affects this tag's CHILDREN, so flip after
            // the stamping decision for the tag itself.
            $selfClosing = $k > $j && $mask[$k - 1] === '/';
            if (!$selfClosing) {
                if ($name === 'script') {
                    $inScript = true;
                } elseif ($name === 'style') {
                    $inStyle = true;
                }
            }

            $stop = $k < $n ? $k + 1 : $n;
            while ($i < $stop) {
                if ($mask[$i] === "\n") {
                    $line++;
                    $lineStart = $i + 1;
                }
                $i++;
            }
        }

        return $inserts;
    }

    // ---- helpers ----

    /** PHP close tag, kept as a constant so no literal `?>` sits in a comment/parse path. */
    private const PHP_CLOSE = '?' . '>';

    private static function at(string $s, int $i, string $needle): bool
    {
        return substr($s, $i, strlen($needle)) === $needle;
    }

    /** True when position $i begins "@$word" as a directive (not extended by ident chars). */
    private static function atDirective(string $s, int $i, string $word): bool
    {
        if ($s[$i] !== '@') {
            return false;
        }
        if (substr($s, $i + 1, strlen($word)) !== $word) {
            return false;
        }
        $after = $i + 1 + strlen($word);
        return !isset($s[$after]) || !self::isIdentChar($s[$after]);
    }

    /** Find the next "@$word" directive at or after $from; returns its '@' offset or false. */
    private static function findDirective(string $s, int $from, string $word)
    {
        $needle = '@' . $word;
        $pos = $from;
        while (($pos = strpos($s, $needle, $pos)) !== false) {
            $after = $pos + strlen($needle);
            if (!isset($s[$after]) || !self::isIdentChar($s[$after])) {
                return $pos;
            }
            $pos = $after;
        }
        return false;
    }

    private static function skipSpaces(string $s, int $i): int
    {
        $n = strlen($s);
        while ($i < $n && ($s[$i] === ' ' || $s[$i] === "\t")) {
            $i++;
        }
        return $i;
    }

    /** Match the balanced ')' for the '(' at $open, honouring PHP string literals. */
    private static function matchParen(string $s, int $open): int
    {
        $n = strlen($s);
        $depth = 0;
        $quote = null;
        for ($i = $open; $i < $n; $i++) {
            $c = $s[$i];
            if ($quote !== null) {
                if ($c === '\\') {
                    $i++;
                    continue;
                }
                if ($c === $quote) {
                    $quote = null;
                }
                continue;
            }
            if ($c === '"' || $c === "'") {
                $quote = $c;
                continue;
            }
            if ($c === '(') {
                $depth++;
            } elseif ($c === ')') {
                $depth--;
                if ($depth === 0) {
                    return $i;
                }
            }
        }
        return $n - 1;
    }

    private static function isIdentStart(string $c): bool
    {
        return ($c >= 'a' && $c <= 'z') || ($c >= 'A' && $c <= 'Z') || $c === '_';
    }

    private static function isIdentChar(string $c): bool
    {
        return ($c >= 'a' && $c <= 'z') || ($c >= 'A' && $c <= 'Z')
            || ($c >= '0' && $c <= '9') || $c === '_';
    }

    private static function isNameChar(string $c): bool
    {
        return ($c >= 'a' && $c <= 'z') || ($c >= 'A' && $c <= 'Z')
            || ($c >= '0' && $c <= '9') || $c === '-' || $c === '_' || $c === ':';
    }
}
