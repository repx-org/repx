CC = gcc
CFLAGS = -Wall

OBJS = main.o file1.o file2.o

mybinary: $(OBJS)
	$(CC) $(CFLAGS) -o mybinary $(OBJS)

%.o: %.c
	$(CC) $(CFLAGS) -c $< -o $@

clean:
	rm -f *.o mybinary
