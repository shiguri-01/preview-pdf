#import "common.typ": int-input

#let default_pages = 10
#let pages = int-input("pages", default_pages)

#set document(title: "pvf bench text")
#set page(
  paper: "a4",
  margin: (x: 22mm, y: 18mm),
  numbering: "1 / 1",
  number-align: bottom + center,
)
#set text(font: "Libertinus Serif", size: 11pt)
#set par(justify: true)

#let paragraph(page_idx, par_idx) = [
  Page #{ page_idx + 1 } of #pages, paragraph #{ par_idx + 1 }.
  This text fixture is visually plain and deterministic.
  It is used for pvf startup, first-page render, page navigation,
  terminal encoding, cache behavior, prefetch scheduling,
  and idle redraw diagnostics.
  The vocabulary is stable across pages: render, cache, layout,
  glyph, viewport, scale, worker, queue, redraw, terminal, latency,
  navigation, first page, next page, previous page, and idle.
]

#for page_idx in range(0, pages) [
  = Text Bench Fixture: Page #{ page_idx + 1 } / #pages

  #for par_idx in range(0, 10) [
    #paragraph(page_idx, par_idx)

  ]

  #if page_idx + 1 < pages [
    #pagebreak()
  ]
]
