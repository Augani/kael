// Request foreground activation only for a sibling process owned by the
// measurement controller. Native phase frame counts still prove repainting.
#import <AppKit/AppKit.h>
#include <errno.h>
#include <limits.h>
#include <libproc.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    char *end = NULL;
    errno = 0;
    long parsed = strtol(argv[1], &end, 10);
    if (errno || !end || *end || parsed <= 0 || parsed > INT_MAX) return 2;
    pid_t target = (pid_t)parsed;
    struct proc_bsdinfo info = {0};
    if (proc_pidinfo(target, PROC_PIDTBSDINFO, 0, &info, sizeof(info)) != sizeof(info)
        || info.pbi_ppid != (uint32_t)getppid()) return 3;
    @autoreleasepool {
        NSRunningApplication *application =
            [NSRunningApplication runningApplicationWithProcessIdentifier:target];
        if (!application) return 4;
        BOOL accepted = [application activateWithOptions:NSApplicationActivateAllWindows];
        printf("{\"activation_accepted\":%s}\n", accepted ? "true" : "false");
        return accepted ? 0 : 4;
    }
}
