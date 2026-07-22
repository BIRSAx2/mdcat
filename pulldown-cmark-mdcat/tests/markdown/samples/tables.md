Explicit line breaks in a cell:

| a              | b |
|----------------|---|
| line1<br>line2 | x |
| b              | c |

Wide cell content wraps to fit the terminal:

| head |
|------|
| This is a long cell that should wrap across multiple lines because its content exceeds the eighty column default terminal width used in these render tests. |

Column widths are distributed proportionally, without breaking a short word apart just to make room for a long neighbour:

| a     | b                                                                                                      |
|-------|--------------------------------------------------------------------------------------------------------|
| short | this cell has quite a lot of content that needs wrapping to fit, unlike its narrow single-word neighbour |
