#import "common.typ": int-input

#let default_pages = 10
#let pages = int-input("pages", default_pages)
#let image_path = sys.inputs.at("image", default: "target/bench/assets/high-res-bench.png")

#set document(title: "pvf bench high resolution image")
#set page(
  width: 216mm,
  height: 216mm,
  margin: 8mm,
  numbering: "1 / 1",
  number-align: bottom + center,
)
#set text(font: "Libertinus Serif", size: 12pt)

#for page_idx in range(0, pages) [
  #align(center + horizon)[
    #image(image_path, width: 100%)
  ]

  #if page_idx + 1 < pages [
    #pagebreak()
  ]
]
