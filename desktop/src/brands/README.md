# Marks kept by hand

One `<slug>.svg` for a brand `simple-icons` does not carry, and for nothing
else. The build reads upstream first, so the day a brand arrives there the file
here stops being used — and `brandmark.test.tsx` asks for it to be deleted.

A file must hold three things, or the build refuses it by name:

- one `<path d="…">` on a **24×24** viewBox,
- `fill="#rrggbb"` on the `<svg>` element: the brand's own colour, which
  `legible()` may lift before it is drawn,
- `<title>` — what the mark is called to a reader.

```svg
<svg viewBox="0 0 24 24" fill="#412991" xmlns="http://www.w3.org/2000/svg">
  <title>Example</title>
  <path d="M12 0…"/>
</svg>
```

Nothing here is required. A slug with no file, upstream or local, draws as a
monogram and the screen stays correct — that is the whole reason this folder
may stay empty.

## What the catalogue is missing today

`openai`, `aws`, `jq`, `ripgrep`, `googleantigravity`.

**The first two were withdrawn from `simple-icons` over trademark, not by
oversight.** Putting a copy back into a public repository is a decision about
what this project may distribute, so it is made by a person with the asset in
front of them — not from memory, and not by a tool that would be guessing at
the shape.
