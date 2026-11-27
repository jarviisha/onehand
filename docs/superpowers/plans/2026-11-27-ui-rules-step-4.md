# UI rules, step 4: the rules, rewritten

Step 4 of `docs/superpowers/specs/2026-11-26-ui-rules-design.md`. Documents only.

- `DESIGN.md`: *Principles*, *Layout* and *Transcript* stay as they are. *Type, space and
  shape*, *Colour and state*, *Components* and *Icons* become rules `T-01` to `T-27` and `C-01`
  to `C-15`. The opening loses "keeps no standing exceptions" and "not values", and gains *How a
  rule reads*.
- `docs/ui-patterns.md`: every section keeps its heading, so the links into it hold, and
  becomes rules `K`, `C-16` to `C-21` (control sizes), `L`, `S` and `W`. *Line rows* joins
  *Lists*. A new section, *Under strain*, holds the layout rules the strain tests check.
- `docs/screens.md` receives what only one screen can meet: the composer as the input surface,
  its popup and its pinned cards' keys; the docks' strip builders; which page is of which kind.
- Each rule: *When*, *Then*, *Unless* where it has one, *Checked by*. A checker is named only
  if it exists: a lint of `clippy.toml`, a guard, a test by its name, a picture, the owner, or
  review.

Decided while writing:
- The seed "a row of actions never wraps" has an *Unless*: a pinned card's answers wrap whole
  before a label is cut, as `card_foot` says and its tests hold.
- The control-size rules keep the family `C` though they stand in `docs/ui-patterns.md`,
  beside the layouts they size.
- "Where no rule decides, do as VS Code does" is written in *How a rule reads*, pending the
  owner's word on the spec's second question.

Review focus: no rule of the old text lost and none invented; every id referenced is defined;
every test, guard and builder named exists; no sentence left that names one screen where a
situation would do.
