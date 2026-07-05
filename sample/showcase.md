# CommonMark

## Markup

![Rust](./rust-logo-128x128.png)

— `mdcat` supports _italic_, **bold**, ~~strikethrough~~, `inline code`, and **_combined_** styles.

> [!TIP]
> Set `$BAT_THEME` to use any bat syntax-highlighting theme for code blocks.

## Code

```rust
fn fibonacci(n: u64) -> u64 {
    match n {
        0 | 1 => n,
        _ => fibonacci(n - 1) + fibonacci(n - 2),
    }
}
```

## Tables

| Language |    Paradigm    |    Typing |
| :------- | :------------: | --------: |
| **Rust** |    Systems     |  _static_ |
| Python   | Multi-paradigm | `dynamic` |
| ~~Java~~ |      OOP       |    static |

## Alerts

> [!NOTE]
> `mdcat` auto-detects dark or light mode from your terminal.

> [!WARNING]
> Sixel inline math can affect line layout in some terminals.

## Math

Inline: $E = mc^2$ and $\sqrt{\pi} = \int_{-\infty}^{\infty} e^{-x^2} dx$
