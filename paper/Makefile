# Palace Paper Makefile

MAIN = palace
LATEX = pdflatex
BIBTEX = bibtex

.PHONY: all clean view

all: $(MAIN).pdf

$(MAIN).pdf: $(MAIN).tex
	$(LATEX) $(MAIN)
	# $(BIBTEX) $(MAIN)
	# $(LATEX) $(MAIN)
	$(LATEX) $(MAIN)

clean:
	rm -f *.aux *.bbl *.blg *.log *.out *.toc *.pdf *.synctex.gz

view: $(MAIN).pdf
	xdg-open $(MAIN).pdf 2>/dev/null || open $(MAIN).pdf
