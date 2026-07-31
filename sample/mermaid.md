# Mermaid diagrams

## Flowchart

```mermaid
flowchart TD
    A[Start] --> B{Decision}
    B -->|Yes| C[Do it]
    B -->|No| D[Skip it]
```

## Sequence diagram

```mermaid
sequenceDiagram
    Alice->>Bob: Hello Bob, how are you?
    Bob-->>Alice: Great!
    Alice->>Bob: See you later
```

## Class diagram

```mermaid
classDiagram
    Animal <|-- Dog
    Animal : +String name
    Animal : +makeSound()
    Dog : +bark()
```

## State diagram

```mermaid
stateDiagram-v2
    [*] --> Idle
    Idle --> Running : start
    Running --> Idle : stop
    Running --> [*]
```

## Entity relationship diagram

```mermaid
erDiagram
    CUSTOMER ||--o{ ORDER : places
    ORDER ||--|{ LINE_ITEM : contains
```

## Gantt chart

```mermaid
gantt
    title Release plan
    dateFormat YYYY-MM-DD
    section Dev
    Design       :a1, 2026-01-01, 5d
    Implement    :a2, after a1, 7d
    section QA
    Test         :a3, after a2, 3d
```

## Pie chart

```mermaid
pie title Browser share
    "Chrome" : 65
    "Safari" : 19
    "Firefox" : 8
    "Other" : 8
```

## User journey

```mermaid
journey
    title Buying coffee
    section Order
      Walk in: 5: Me
      Order: 3: Me
    section Wait
      Wait for coffee: 2: Me
    section Enjoy
      Drink coffee: 5: Me
```

## Git graph

```mermaid
gitGraph
    commit
    branch develop
    checkout develop
    commit
    checkout main
    merge develop
    commit
```

## Mindmap

```mermaid
mindmap
    root((mdcat))
        Rendering
            Syntax highlighting
            Inline images
        Formats
            Math
            Mermaid
```

## Timeline

```mermaid
timeline
    title mdcat feature history
    2020 : Initial release
    2023 : Named themes
    2026 : Mermaid diagrams
```

## Quadrant chart

```mermaid
quadrantChart
    title Feature priority
    x-axis Low Impact --> High Impact
    y-axis Low Effort --> High Effort
    Diagrams: [0.8, 0.4]
    Themes: [0.6, 0.3]
```
