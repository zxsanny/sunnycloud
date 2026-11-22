/* Probe of check-decoder-seccomp.sh (AZ-3855 AC-6, IT-10-15): runs as the container user and prints, one `name=value`
 * per line, what the seccomp profile and the kernel answer when a process of the container tries to attach to or read
 * another one. Built statically by the check into a temp dir and bind-mounted read-only; never part of an image. */
#define _GNU_SOURCE
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/ptrace.h>
#include <sys/syscall.h>
#include <sys/uio.h>
#include <sys/wait.h>
#include <unistd.h>

static char shared_buffer[64] = "probe";

static const char *answer(long rc, int err) {
    if (rc >= 0) return "OK";
    switch (err) {
    case EPERM: return "EPERM";
    case EACCES: return "EACCES";
    case ENOSYS: return "ENOSYS";
    default: return strerror(err);
    }
}

static void status_field(const char *name) {
    char line[256];
    size_t len = strlen(name);
    FILE *status = fopen("/proc/self/status", "r");
    if (!status) {
        printf("%s=unreadable\n", name);
        return;
    }
    while (fgets(line, sizeof line, status)) {
        if (strncmp(line, name, len) == 0 && line[len] == ':') {
            char *value = line + len + 1;
            value += strspn(value, " \t");
            value[strcspn(value, "\n")] = 0;
            printf("%s=%s\n", name, value);
            fclose(status);
            return;
        }
    }
    fclose(status);
    printf("%s=absent\n", name);
}

int main(void) {
    pid_t sleeper = fork();
    if (sleeper < 0) return 2;
    if (sleeper == 0) {
        pause();
        _exit(0);
    }

    pid_t tracee = fork();
    if (tracee < 0) return 2;
    if (tracee == 0) {
        long rc = syscall(SYS_ptrace, PTRACE_TRACEME, 0, 0, 0);
        _exit(rc < 0 ? errno : 0);
    }
    int wstatus = 0;
    waitpid(tracee, &wstatus, 0);
    int err = WIFEXITED(wstatus) ? WEXITSTATUS(wstatus) : -1;
    printf("ptrace_traceme=%s\n", err == 0 ? "OK" : answer(-1, err));

    long rc = syscall(SYS_ptrace, PTRACE_ATTACH, sleeper, 0, 0);
    printf("ptrace_attach=%s\n", answer(rc, errno));
    if (rc == 0) {
        waitpid(sleeper, &wstatus, 0);
        syscall(SYS_ptrace, PTRACE_DETACH, sleeper, 0, 0);
    }

    char local[64];
    struct iovec local_vec = {local, sizeof local};
    struct iovec remote_vec = {shared_buffer, sizeof shared_buffer};
    rc = syscall(SYS_process_vm_readv, sleeper, &local_vec, 1, &remote_vec, 1, 0);
    printf("process_vm_readv=%s\n", answer(rc, errno));
    rc = syscall(SYS_process_vm_writev, sleeper, &local_vec, 1, &remote_vec, 1, 0);
    printf("process_vm_writev=%s\n", answer(rc, errno));

    status_field("Seccomp");
    status_field("NoNewPrivs");
    kill(sleeper, SIGKILL);
    waitpid(sleeper, &wstatus, 0);
    return 0;
}
