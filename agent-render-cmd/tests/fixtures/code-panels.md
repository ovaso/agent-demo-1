## Syntax highlighting

```python
def greet(name: str) -> str:
    # Reuse syntax state across streamed lines.
    return f"Hello, {name}!"

print(greet("world"))
```

```diff
- timeout = 10
+ timeout = 30
```

```
<tag> &amp; **literal text**
```

Normal output resumes here.
