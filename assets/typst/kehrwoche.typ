// Kehrwoche schedule PDF template.
//
// Layout: title with the logo floating in the top-right corner (independent
// of the document flow), the year's weeks split into two balanced columns,
// and a QR band at the bottom with QR codes for the PDF itself and the iCal
// feed (rendered only when a public URL is configured; otherwise a reserved
// box stands in for each). Rows from the previous/next year that only exist
// to balance the columns are greyed out and marked `info`.
//
// The wrapper (see pdf.rs) writes logo.svg / qr.svg / qr_ical.svg next to
// this file and passes the left/right row arrays plus the absolute PDF and
// iCal links. Column headers and QR captions are localized per request.

#let schedule_table(rows, col_from, col_to) = table(
  table.header([*W*], [*#col_from*], [*#col_to*], [*Name*]),
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

#let kehrwoche(building_name: str, year: int, left_rows: array, right_rows: array, pdf_url: str, ical_url: str, col_from: str, col_to: str, qr_pdf_caption: str, qr_ical_caption: str) = [
  // Logo floats in the top-right corner, right-aligned to the page margin
  // and independent of the document flow (so its size never pushes the
  // schedule down). dy lifts it into the top margin so it stays clear of
  // the table header.
  #place(top + right, dy: -0.9cm)[
    #image("logo.svg", height: 2.7cm)
  ]

  #align(left)[
    #text(size: 16pt, weight: "bold")[Kehrwoche #year #building_name]
  ]

  #v(7pt)

  #set text(size: 10.5pt)
  #columns(2, gutter: 1.1cm)[
    #schedule_table(left_rows, col_from, col_to)
    #colbreak()
    #schedule_table(right_rows, col_from, col_to)
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
        #text(7.5pt, fill: luma(120))[#qr_pdf_caption]
      ]
    ],
    [
      // QR code for the iCal feed so residents can subscribe to the
      // calendar directly from the printed plan.
      #align(center)[
        #if ical_url.len() > 0 {
          [#image("qr_ical.svg", width: 2.35cm)]
        } else {
          [#reserved_box]
        }
        #v(2pt)
        #text(7.5pt, fill: luma(120))[#qr_ical_caption]
      ]
    ],
  )
]
