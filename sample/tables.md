# Tables

## Line breaks in cells

`<br>` starts a new line inside a cell instead of being dropped or printed literally:

| Tool  | Description                                 |
|:------|:---------------------------------------------|
| mdcat | Renders Markdown<br>right in your terminal   |
| bat   | `cat` clone<br>with syntax highlighting      |

## Wide cells wrap to fit the terminal

A cell wider than the terminal wraps across multiple lines instead of overflowing:

| Feature       | Notes                                                                                                          |
|:--------------|:-----------------------------------------------------------------------------------------------------------|
| Table layout  | Column widths are distributed proportionally and shrunk to fit the terminal, wrapping cell content as needed instead of letting it run off the edge of the screen. |

## Proportional column widths

Columns shrink proportionally to their content, but never break a short word apart just to make room for a long neighbour:

| Tool  | Purpose                                                                                       |
|:------|:-----------------------------------------------------------------------------------------------|
| mdcat | Render Markdown in the terminal, with syntax highlighting, images, and now wrapped table cells |
| bat   | `cat` with syntax highlighting, Git integration, and automatic paging                          |
