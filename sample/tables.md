# Tables

## Basic table

| Name       | Version | License   |
|:-----------|:-------:|----------:|
| mdcat      | 2.10.0  | MPL-2.0   |
| pulldown-cmark | 0.12 | MIT   |
| syntect    | 5.3     | MIT       |
| two-face   | 0.5     | MIT       |

## Inline markup in cells

| Feature         | Status          | Notes                        |
|:----------------|:---------------:|:-----------------------------|
| **Bold**        | ✓ works         | Use `**text**`               |
| *Italic*        | ✓ works         | Use `*text*` or `_text_`     |
| ~~Strikethrough~~ | ✓ works       | Use `~~text~~`               |
| `inline code`   | ✓ works         | Use `` `code` ``             |
| ***Bold italic*** | ✓ works       | Nesting works too            |
| ~~**both**~~    | ✓ works         | Strike + bold                |

## Mixed alignments

| Left          |    Center     |         Right |
|:--------------|:-------------:|--------------:|
| `alpha`       |   *middle*    |    **right**  |
| longer text   |    ~~gone~~   |          1234 |
| short         |   ***wow***   |             0 |

## Table without body

| Column A | Column B | Column C |
|----------|----------|----------|

## Links in cells

| Tool                        | Purpose                  |
|:----------------------------|:-------------------------|
| [mdcat](https://github.com/BIRSAx2/mdcat) | Render Markdown in the terminal |
| [bat](https://github.com/sharkdp/bat)     | `cat` with syntax highlighting  |
| [delta](https://github.com/dandavison/delta) | Syntax-highlighting pager for git |
