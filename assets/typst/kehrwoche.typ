#let kehrwoche(plan_name: str, year: int, rows: array) = [
  = Kehrwoche #year #plan_name

  #columns(2)[
    #table(
      table.header([*W*], [*von*], [*bis*], [*Name*]),
      columns: (auto, auto, auto, 1fr),
      stroke: none,
      ..for row in rows {
        (
          [#row.week],
          [#row.start],
          [#row.end],
          [#row.name],
        )
      },
    )
  ]
]
