bin/drat-trim: vendor/drat-trim/drat-trim.c
	@mkdir -p bin
	gcc $< -std=c99 -O2 -o $@
