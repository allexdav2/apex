#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* stdin-driven branching program for APEX integration tests. */
int classify(int x) {
    if (x < 0) return -1;
    if (x == 0) return 0;
    if (x > 100) return 2;
    return 1;
}

int main(void) {
    char buf[64];
    if (fgets(buf, sizeof(buf), stdin) == NULL) {
        return 1;
    }
    int val = atoi(buf);
    int cls = classify(val);
    printf("class=%d\n", cls);
    return 0;
}
