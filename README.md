# mdcat

Fancy `cat` for Markdown (that is, [CommonMark][]):

```
$ mdcat sample.md
```

![mdcat showcase with different colour themes][sxs]

mdcat in [Ghostty], with `--theme catppuccin-mocha`, `--theme dracula`,
and `--theme nord` (left to right), using [JetBrains Mono].

[CommonMark]: http://commonmark.org
[JetBrains Mono]: https://www.jetbrains.com/lp/mono/
[sxs]: ./screenshots/side-by-side.png

## Features

`mdcat` works best with [iTerm2], [WezTerm], [kitty], and [Ghostty], and a good terminal font with italic characters.
Then it

- nicely renders all basic CommonMark syntax, plus definition lists,
- renders footnotes and inline markup in table cells,
- highlights code blocks with syntax definitions from [bat] (TOML, TypeScript, Dockerfile, Zig, Nix, and more),
  using the colour theme of your choice — including any theme [bat] supports via `$BAT_THEME`,
- renders inline and display math, as PNGs through the iTerm2 or kitty image protocol, and as Unicode substitutions otherwise,
- renders [Mermaid][mermaid] diagrams (flowcharts, sequence diagrams, and more) in fenced ` ```mermaid ` code blocks,
  as PNGs through an image protocol, and as Unicode diagrams otherwise,
- renders [GFM alerts][gfm-alerts] (`[!NOTE]`, `[!TIP]`, `[!WARNING]`, etc.) with coloured borders and icons,
- shows [links][osc8], and also images inline in supported terminals (see above, where "Rust" is a clickable link!),
- adds jump marks for headings in [iTerm2] (jump forwards and backwards with <key>⇧⌘↓</key> and <key>⇧⌘↑</key>),
- ships eight built-in colour themes (`catppuccin-mocha`, `catppuccin-latte`, `gruvbox-dark`, `gruvbox-light`,
  `dracula`, `nord`, `solarized-dark`, `solarized-light`) plus auto dark/light detection,
  and lets you fully customise colours, heading markers, and GFM alert icons/labels via
  `~/.config/mdcat/config.toml` (see [config.toml.example](./config.toml.example)),
- can render typographic punctuation (curly quotes, en/em dashes, an ellipsis) with `--smart-punctuation`,
- can watch a file and re-render it on every save with `--watch`, for a live preview while editing,
- can fuzzy-find a Markdown file to render, via `mdpick` (see below), if you have [fzf] installed.

| Terminal           | Basic syntax | Syntax highlighting | Images | Math  | Jump marks |
| :----------------- | :----------: | :-----------------: | :----: | :---: | :--------: |
| Basic ANSI¹        |      ✓       |          ✓          |        |  ✓³   |            |
| Windows 10 console |      ✓       |          ✓          |        |  ✓³   |            |
| [iTerm2]           |      ✓       |          ✓          |   ✓²   |  ✓³   |     ✓      |
| [kitty]            |      ✓       |          ✓          |   ✓²   |  ✓³   |            |
| [WezTerm]          |      ✓       |          ✓          |   ✓²   |  ✓³   |            |
| [VSCode]           |      ✓       |          ✓          |        |  ✓³   |            |
| [Ghostty]          |      ✓       |          ✓          |   ✓²   |  ✓³   |            |
| [foot]⁴            |      ✓       |          ✓          |   ✓²   |  ✓³‧⁵ |            |
| [xterm]⁴           |      ✓       |          ✓          |   ✓²   |  ✓³‧⁵ |            |

1. mdcat requires that the terminal supports strikethrough formatting and [inline links][osc8].
   This includes most modern terminal emulators, such as Windows Terminal, KDE Konsole, or anything based on VTE, GNOME's terminal emulation library.
   But mdcat likely won't work well on old terminals that lack these features (e.g. the Linux text console).
2. SVG images are rendered with [resvg], see [SVG support].
3. On terminals with the iTerm2, kitty, or Sixel image protocol, math is rendered as PNG images.
   Otherwise mdcat uses Unicode substitutions.
4. Uses Sixel for images, which is also supported by many other terminals.
   The capability detection logic would need to be extended to also provide the feature on other terminals, though.
5. Inline math with Sixel breaks the layout, as it causes vertical scrolling that is not taking into account.

Not supported:

- Text wrapping inside table cells.

[bat]: https://github.com/sharkdp/bat
[gfm-alerts]: https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax#alerts
[osc8]: https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda
[iterm2]: https://www.iterm2.com
[WezTerm]: https://wezfurlong.org/wezterm/
[kitty]: https://sw.kovidgoyal.net/kitty/
[resvg]: https://github.com/RazrFalcon/resvg
[SVG support]: https://github.com/RazrFalcon/resvg#svg-support
[VSCode]: https://code.visualstudio.com/
[Ghostty]: https://mitchellh.com/ghostty
[foot]: https://codeberg.org/dnkl/foot
[xterm]: https://invisible-island.net/xterm/xterm.html
[fzf]: https://github.com/junegunn/fzf
[mermaid]: https://mermaid.js.org

## Usage

Try `mdcat --help` or read the [mdcat(1)](./mdcat.1.adoc) manpage.

To pick a colour theme:

```console
$ mdcat --theme catppuccin-mocha sample.md
$ MDCAT_THEME=dracula mdcat sample.md
$ mdcat --list-themes  # preview every built-in theme
```

To customise colours, heading markers, and GFM alert icons/labels, or set defaults for flags like
`--margin`, `--smart-punctuation`, or `--columns`, create `~/.config/mdcat/config.toml`.
See [config.toml.example](./config.toml.example) for a fully annotated example, or the
[mdcat(1)](./mdcat.1.adoc) manpage's "Configuration file" section for the reference.

To use a [bat] syntax-highlighting theme for code blocks:

```console
$ BAT_THEME="Catppuccin Mocha" mdcat sample.md
```

To get a live preview while editing:

```console
$ mdcat --watch sample.md
```

See [sample/math.md](./sample/math.md) for math examples, [sample/mermaid.md](./sample/mermaid.md) for Mermaid
diagrams, [sample/alerts.md](./sample/alerts.md) for GFM alerts, and [sample/tables.md](./sample/tables.md) for
table rendering.

To fuzzy-find and open a Markdown file below the current directory (requires [fzf]):

```console
$ mdpick
$ mdpick docs  # search below docs instead
```

### `mdless` and `mdpick`

`mdcat` looks at the name it was invoked as (`argv[0]`) to decide how to behave:

- Invoked as `mdless`, it automatically paginates output, as if `--paginate` were given.
- Invoked as `mdpick`, it skips straight to the interactive picker described above, instead of expecting a file argument.

Both are the same binary as `mdcat`, just under a different name, so you get them for free by linking or copying the binary:

```console
$ ln -s "$(command -v mdcat)" ~/.local/bin/mdless
$ ln -s "$(command -v mdcat)" ~/.local/bin/mdpick
```

(make sure `~/.local/bin`, or wherever you put the links, is on `$PATH`). A hardlink or plain copy works just as well as a symlink.

## Installation

- [Release binaries](https://github.com/BIRSAx2/mdcat/releases/) built on Github Actions.
  - These binaries are built from Git source on Github Actions; you find provenance attestations at <https://github.com/BIRSAx2/mdcat/attestations>.
- Package managers — `mdcat` is available from several package managers:

  | Package manager | Command |
  | --- | --- |
  | [Homebrew](https://formulae.brew.sh/formula/mdcat) (macOS/Linux) | `brew install mdcat` |
  | [AUR](https://aur.archlinux.org/packages/mdcat-bin) (Arch Linux) | `paru -S mdcat-bin` |
  | [Nixpkgs](https://search.nixos.org/packages?query=mdcat) | `nix-env -iA nixpkgs.mdcat` |
  | [MacPorts](https://ports.macports.org/port/mdcat/) | `sudo port install mdcat` |
  | [FreeBSD Ports](https://www.freshports.org/sysutils/mdcat/) | `pkg install mdcat` |
  | [Void Linux](https://voidlinux.org/packages/?q=mdcat) | `sudo xbps-install mdcat` |
  | [MSYS2](https://packages.msys2.org/base/mingw-w64-mdcat) (Windows) | `pacman -S mingw-w64-x86_64-mdcat` |

  See [Repology](https://repology.org/project/mdcat/versions) for the full list of packages and their versions.
- You can also build `mdcat` manually with `cargo install mdcat` (see below for details).

`mdcat` can be linked or copied to `mdless` or `mdpick` (see [`mdless` and `mdpick`](#mdless-and-mdpick) above).

## Building

Run `cargo build --release`.

Building requires `libcurl`.

## Packaging

When packaging `mdcat` you may wish to include the following additional artifacts:

- A symlink or hardlink from `mdless` to `mdcat` (see above), and, if you want to ship the picker, likewise from `mdpick` to `mdcat`. `mdpick` additionally needs [fzf] installed to work; consider depending on it in your package.
- Shell completions for relevant shells, by invoking `mdcat --completions` after building, e.g.

  ```console
  $ mdcat --completions fish > /usr/share/fish/vendor_completions.d/mdcat.fish
  $ mdcat --completions bash > /usr/share/bash-completion/completions/mdcat
  $ mdcat --completions zsh > /usr/share/zsh/site-functions/_mdcat
  # Same for mdless and mdpick if you include them
  $ mdless --completions fish > /usr/share/fish/vendor_completions.d/mdless.fish
  $ mdless --completions bash > /usr/share/bash-completion/completions/mdless
  $ mdless --completions zsh > /usr/share/zsh/site-functions/_mdless
  $ mdpick --completions fish > /usr/share/fish/vendor_completions.d/mdpick.fish
  $ mdpick --completions bash > /usr/share/bash-completion/completions/mdpick
  $ mdpick --completions zsh > /usr/share/zsh/site-functions/_mdpick
  ```

- A build of the man page `mdcat.1.adoc`, using [AsciiDoctor]:

  ```console
  $ asciidoctor -b manpage -a reproducible -o /usr/share/man/man1/mdcat.1 mdcat.1.adoc
  $ gzip /usr/share/man/man1/mdcat.1
  # If you include mdless and/or mdpick as above, you may also want to support their man pages
  $ ln -s mdcat.1.gz /usr/share/man/man1/mdless.1.gz
  $ ln -s mdcat.1.gz /usr/share/man/man1/mdpick.1.gz
  ```

[AsciiDoctor]: https://asciidoctor.org/

## Troubleshooting

`mdcat` can output extensive tracing information when asked to.
Run `mdcat` with `$MDCAT_LOG=trace` for complete tracing information, or with `$MDCAT_LOG=mdcat::render=trace` to trace only rendering.

## License

Copyright Sebastian Wiesner <sebastian@swsnr.de> and Mouhieddine Sabir <me@mouhieddine.dev>

Currently maintained by [BIRSAx2](https://github.com/BIRSAx2).

Binaries are subject to the terms of the Mozilla Public
License, v. 2.0, see [LICENSE](LICENSE).

Most of the source is subject to the terms of the Mozilla Public
License, v. 2.0, see [LICENSE](LICENSE), unless otherwise noted;
some files are subject to the terms of the Apache 2.0 license,
see <http://www.apache.org/licenses/LICENSE-2.0>
