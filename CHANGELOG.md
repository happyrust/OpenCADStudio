# Changelog

## [Unreleased]

### Added

- Options → Lineweight: “P&ID paper style scale” (`pid_paper_style_scale` in
  settings, default 1.0, clamped to 0.25..=4.0 on load). The next `.pid` open
  multiplies style-declared dash lengths, lineweights, and point-marker
  strokes by that factor. Text height is unchanged. `load_file` stays at 1.0.
