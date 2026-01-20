# Palace Paper

Academic paper describing Palace's architecture and workflows.

## Building

```bash
make        # Build PDF
make clean  # Remove build artifacts
make view   # Build and open PDF
```

## Requirements

- LaTeX distribution (texlive-full recommended)
- pdflatex
- TikZ package for diagrams

## Structure

- `palace.tex` - Main paper source
- `Makefile` - Build automation

## Updating

This paper is a living document.
Update it as Palace evolves and deployment data becomes available.

Sections to update with real data:
- Section 6 (Preliminary Results) - Add metrics from deployments
- Section 8 (Future Work) - Move completed items to main sections
