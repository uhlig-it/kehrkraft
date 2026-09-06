// Kehrwoche schedule PDF template.
//
// Layout: title with the logo floating in the top-right corner (independent
// of the document flow), the year's weeks split into two balanced columns,
// and a QR band at the bottom (current PDF link + a reserved slot for a
// future QR code). Rows from the previous/next year that only exist to
// balance the columns are greyed out and marked `info`.
//
// The wrapper (see pdf.rs) writes logo.svg / qr.svg next to this file and
// passes the left/right row arrays plus the absolute PDF link.

#let schedule_table(rows) = table(
  table.header([*W*], [*von*], [*bis*], [*Name*]),
  columns: (auto, auto, auto, 1fr),
  stroke: none,
  inset: (x: 4pt, y: 6.3pt),
  ..rows.map(row => (
    if row.info { text(fill: luma(175))[#row.week] } else { [#row.week] },
    if row.info { text(fill: luma(175))[#row.start] } else { [#row.start] },
    if row.info { text(fill: luma(175))[#row.end] } else { [#row.end] },
    if row.info { text(fill: luma(175))[#row.name] } else { [#row.name] },
  )).flatten(),
)

#let reserved_box = box(
  width: 2.35cm,
  height: 2.35cm,
  stroke: (paint: luma(180), thickness: 0.5pt, dash: "dashed"),
  radius: 4pt,
)

#let kehrwoche(building_name: str, year: int, left_rows: array, right_rows: array, pdf_url: str) = [
  // Logo floats in the top-right corner, independent of the document flow
  // (so its size never pushes the schedule down). dy lifts it into the top
  // margin so it stays clear of the table header.
  #place(top + right, dx: -0.35cm, dy: -0.9cm)[
    #image("logo.svg", height: 2.2cm)
  ]

  #align(left)[
    #text(size: 16pt, weight: "bold")[Kehrwoche #year #building_name]
  ]

  #v(7pt)

  #set text(size: 10.5pt)
  #columns(2, gutter: 1.1cm)[
    #schedule_table(left_rows)
    #colbreak()
    #schedule_table(right_rows)
  ]

  #v(4pt)
  #line(length: 100%, stroke: 0.5pt + luma(220))
  #v(7pt)

  #grid(
    columns: (1fr, 1fr),
    gutter: 1.1cm,
    [
      #align(center)[
        #if pdf_url.len() > 0 {
          [#image("qr.svg", width: 2.35cm)]
        } else {
          [#reserved_box]
        }
        #v(2pt)
        #text(7.5pt, fill: luma(120))[Kehrwoche-PDF per QR öffnen]
      ]
    ],
    [
      // Reserved space for a second QR code that will be added later.
      #align(center)[
        #reserved_box
        #v(2pt)
        #text(7.5pt, fill: luma(120))[Platz für weiteren QR-Code]
      ]
    ],
  )
]
