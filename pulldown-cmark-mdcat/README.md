# pulldown-cmark-mdcat

[![Crates.io](https://img.shields.io/crates/v/pulldown-cmark-mdcat)](https://crates.io/crates/pulldown-cmark-mdcat)
[![docs.rs](https://img.shields.io/docsrs/pulldown-cmark-mdcat)](https://docs.rs/pulldown-cmark-mdcat)

Render [pulldown-cmark] events to a TTY.

This library backs the [mdcat] tool, and makes its rendering available to other crates.

It supports:

- All common mark syntax.
- Standard ANSI formatting with OCS-8 hyperlinks.
- Inline images on terminal emulators with either the iTerm2 or the Kitty protocol.
- Footnotes.
- Math events, rendered as PNGs with the iTerm2 or Kitty protocol and as Unicode substitutions otherwise.
- Jump marks in iTerm2.

Math image rendering falls back to Unicode substitutions when the terminal or expression is not supported.

## Ratatui

Enable the `ratatui` feature to render markdown inside a Ratatui widget:

```rust
use pulldown_cmark_mdcat::ratatui::{MdcatWidget, MdcatWidgetState};

let mut state = MdcatWidgetState::new();

frame.render_stateful_widget(MdcatWidget::new(markdown), area, &mut state);
```

For direct `Text` output, use `text_from_str` or `text_from_read`. For custom themes, syntax sets,
syntax themes, base directories, or resource access, use `Renderer` with explicit `RenderOptions`.

Links are rendered as visible references because Ratatui `Text` does not carry hyperlink targets.
Image protocol probing is available through `MdcatWidgetState::detect_images`; call it after
entering the alternate screen if your app wants the widget to render supported image resources.

Run the demo executable with:

```console
cargo run -p pulldown-cmark-mdcat --example ratatui --features ratatui-demo -- README.md
```

[mdcat]: https://github.com/BIRSAx2/mdcat
[pulldown-cmark]: https://github.com/raphlinus/pulldown-cmark

## License

Copyright Sebastian Wiesner <sebastian@swsnr.de> and Mouhieddine Sabir <me@mouhieddine.dev>

Binaries are subject to the terms of the Mozilla Public
License, v. 2.0, see [LICENSE](LICENSE).

Most of the source is subject to the terms of the Mozilla Public
License, v. 2.0, see [LICENSE](LICENSE), unless otherwise noted;
some files are subject to the terms of the Apache 2.0 license,
see <http://www.apache.org/licenses/LICENSE-2.0>
