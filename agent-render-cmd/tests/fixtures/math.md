# LaTeX rendering

Inline: $E=mc^2$, $x_i^{n+1}$, $\alpha + \beta$, $\frac{a+b}{c}$, $\sqrt[3]{27}$.

## Fraction and root

$$
\frac{-b \pm \sqrt{b^2-4ac}}{2a}
$$

## Matrix

$$
\begin{bmatrix}
1 & 2 \\
3 & 4
\end{bmatrix}
$$

## Sum and integral

$$
\sum_{i=1}^n x_i + \int_0^1 x^2 dx
$$

## Cases

$$
\begin{cases}x^2 & x>0 \\ 0 & x=0\end{cases}
$$

> Quoted equation:
> $$
> \frac{x}{y}
> $$

- Item:
  $$
  \sqrt[3]{27}
  $$

| Expression | Meaning |
|---|---|
| $H_2O$ | water |
| $\frac{a}{b}$ | fraction |

Literal: `$x^2$`, `\frac{a}{b}`, escaped \$5 and ordinary $5 and $10.

Unknown: $\notacommand{x}$; incomplete argument: $\frac{a}$.

```latex
\frac{a}{b}
$x^2$
```

After math: **normal Markdown**.
