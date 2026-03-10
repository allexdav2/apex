CC ?= gcc
CFLAGS ?= -Wall -Wextra -O0

all: main

main: main.c
	$(CC) $(CFLAGS) -o main main.c

clean:
	rm -f main

.PHONY: all clean
