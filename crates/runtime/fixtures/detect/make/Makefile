.PHONY: clean build
MARK := $(shell touch detect-spawn-marker)
all build: input.txt
	touch detect-spawn-marker
%.o: %.c
	cc $<
$(DYNAMIC):
	touch detect-spawn-marker
