# JAFIZ suite format

One `.md` file per suite. Ordinary Markdown — it renders on GitHub and in
any editor. JAFIZ never rewrites this file: run progress lives in
`.runs/<name>.json`, right beside it.

## Rules

1. The first `# ` is the suite's title (without one, the file name is used).
2. `key: value` lines between the `# ` and the first `## ` are the suite's metadata.
3. Each `## ` opens a scenario. Start with the id (`SC-01`, `CT-7`, `REG-102`)
   followed by `·`, `-`, `—`, `:` or a space. Without an explicit id, JAFIZ
   generates one from the position — and a scenario inserted in the middle
   shifts the ids below it.
4. Under the `## `, before the first step: `tags: a, b, c` and
   `pré: ...` (or `pre:`, `precondição:`, `precondition:`, `given:`).
5. Loose text before the first step becomes the scenario's description.
6. Each list item (`1.`, `1)`, `-`, `*`) is a step. The file's numbering
   is ignored — steps are renumbered starting at 1 within each scenario.
7. Within a step, the first `→`, `->` or `=>` separates the **action** from
   the **expected result**.
8. An indented line (2+ spaces) right after a step continues that step.
9. Blank lines, `<!-- -->`, `>` and `---` are ignored.
10. UTF-8.

## Example

```markdown
# Checkout — regression v2.3
product: Tolky Store

## SC-01 · Purchase with approved card
tags: checkout, payment
precondition: user logged in; cart with 1 item

1. Open the cart → the item shows the correct price
2. Click "Checkout" → the payment screen opens
3. Enter the test card 4111 1111 1111 1111
   → the "Pay" button becomes enabled
```

## Flow

- Write the `.md` and run `jafiz check <file>` to validate it.
- The user runs through the steps in `jafiz-gui` and marks each one.
- Run `jafiz report` to read what passed, what failed, and the notes.
