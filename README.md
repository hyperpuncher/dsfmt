# dsfmt

Datastar attribute formatter — reflows `data-*` attributes and template expressions in HTML, JSX, and TSX files.

## Install

```sh
cargo install --git https://github.com/hyperpuncher/dsfmt
```

## Usage

```sh
# Format a file (prints to stdout)
dsfmt src/components/slider.tsx

# Write changes back
dsfmt --write src/components/slider.tsx

# Format all supported files in a directory
dsfmt --write src/

# Check if files are formatted (exit code 1 if changes needed)
dsfmt --check src/

# Read from stdin
cat file.html | dsfmt
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--line-width` | 90 | Maximum line width before splitting |
| `--use-spaces` | false | Use spaces instead of tabs |
| `--tab-width` | 4 | Number of spaces per indent level |
| `--write`, `-w` | false | Write changes to files |
| `--check` | false | Exit 1 if formatting would change |

## What it does

- Splits elements with 2+ `data-*` attributes across multiple lines (when they don't fit on one)
- Splits multi-statement template literal expressions (`data-effect={\`a=1; b=2\`}`)
- Splits object/array values in quoted attributes (`data-signals="{a:1, b:2}"`)
- Normalizes `,` to `;` in template statements
- Preserves everything else byte-for-byte

## Supported file types

HTML, JSX, TSX, Templ, HEEx, Blade
