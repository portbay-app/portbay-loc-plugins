<?php

declare(strict_types=1);

namespace Portbay\BladeStamper;

use Illuminate\Support\ServiceProvider;
use Illuminate\View\Compilers\BladeCompiler;

/**
 * Registers the Blade source-location stamper as the FIRST Blade compilation
 * pass, so every rendered host element carries `data-pb-loc="file:line:col"`
 * pointing at its authored `.blade.php` span — the same instrumentation the
 * `@portbay/*` JS plugins provide for JSX/Vue/Svelte/Astro.
 *
 * Dev-only, by design (mirrors the JS plugins' NODE_ENV gate):
 *   - emits UNLESS the app is in the `production` environment;
 *   - `PORTBAY_LOC` env var forces it on (`1`) or off (`0`) regardless.
 * The attribute never reaches a production render, so no DOM bloat and no
 * leaking of local file paths.
 */
final class PortbayLocServiceProvider extends ServiceProvider
{
    public function boot(): void
    {
        if (!$this->enabled()) {
            return;
        }

        // Wire the compiler whether it is already resolved or resolved later.
        $this->callAfterResolving('blade.compiler', function ($blade): void {
            if (!$blade instanceof BladeCompiler) {
                return;
            }
            // `prepareStringsForCompilationUsing` runs before Blade mutates the
            // template, so line/col are read from pristine source. It was added
            // in Laravel 10.15; on anything older we no-op (text-search stays).
            if (!method_exists($blade, 'prepareStringsForCompilationUsing')) {
                return;
            }

            $base = $this->basePath();
            $blade->prepareStringsForCompilationUsing(static function (string $value) use ($blade, $base): string {
                $path = $blade->getPath();
                if (!is_string($path) || $path === '') {
                    return $value;
                }
                return PbLocStamper::stamp($value, self::relativePath($path, $base));
            });
        });
    }

    /** Emit unless production; PORTBAY_LOC forces the decision either way. */
    private function enabled(): bool
    {
        $force = getenv('PORTBAY_LOC');
        if ($force !== false && $force !== '') {
            return filter_var($force, FILTER_VALIDATE_BOOLEAN);
        }

        $app = $this->app;
        // Prefer the framework's environment check; fall back to APP_ENV.
        if (is_object($app) && method_exists($app, 'environment')) {
            return !$app->environment('production');
        }
        return (getenv('APP_ENV') ?: 'production') !== 'production';
    }

    private function basePath(): string
    {
        $app = $this->app;
        if (is_object($app) && method_exists($app, 'basePath')) {
            return rtrim((string) $app->basePath(), '/\\');
        }
        return rtrim((string) (getenv('APP_BASE_PATH') ?: getcwd()), '/\\');
    }

    /** Project-root-relative POSIX path; absolute (normalised) when outside the root. */
    private static function relativePath(string $path, string $base): string
    {
        $path = str_replace('\\', '/', $path);
        $base = str_replace('\\', '/', $base);
        if ($base !== '' && str_starts_with($path, $base . '/')) {
            return substr($path, strlen($base) + 1);
        }
        return ltrim($path, '/');
    }
}
