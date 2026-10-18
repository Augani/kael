// Read-only per-process counters from the macOS SDK's versioned libproc ABI.
#include <libproc.h>
#include <mach/mach_time.h>
#include <sys/resource.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <errno.h>
#include <stddef.h>
#include <string.h>

// Keep the complete V4 ABI prefix and every original counter unchanged.
struct collector_usage {
    struct rusage_info_v4 common;
    int flavor;
    int v6_sdk_available;
    int v6_errno;
    uint64_t energy_nj;
    uint64_t penergy_nj;
};

static int read_process_usage(int pid, struct collector_usage *result) {
    memset(result, 0, sizeof(*result));
#if defined(RUSAGE_INFO_V6)
    _Static_assert(offsetof(struct rusage_info_v6, ri_flags) == sizeof(struct rusage_info_v4),
                   "V6 must preserve the complete V4 prefix");
    result->v6_sdk_available = 1;
    struct rusage_info_v6 v6 = {0};
    if (proc_pid_rusage(pid, RUSAGE_INFO_V6, (rusage_info_t *)&v6) == 0) {
        memcpy(&result->common, &v6, sizeof(result->common));
        result->flavor = RUSAGE_INFO_V6;
        result->energy_nj = v6.ri_energy_nj;
        result->penergy_nj = v6.ri_penergy_nj;
        return 0;
    }
    // Apple's published proc_get_rusage rejects an unknown flavor with EINVAL.
    // Permission, process-exit and other errors remain failures, not fallback.
    result->v6_errno = errno;
    if (result->v6_errno != EINVAL) return -1;
#endif
    if (proc_pid_rusage(pid, RUSAGE_INFO_V4, (rusage_info_t *)&result->common) != 0) return -1;
    result->flavor = RUSAGE_INFO_V4;
    return 0;
}

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    struct collector_usage collected;
    if (read_process_usage(atoi(argv[1]), &collected) != 0) return 1;
    struct rusage_info_v4 usage = collected.common;
    mach_timebase_info_data_t timebase = {0};
    if (mach_timebase_info(&timebase) != KERN_SUCCESS || timebase.denom == 0) return 1;
    // libproc's CPU fields are Mach absolute ticks, including on Apple Silicon.
    // Widen before multiplying so long-lived processes cannot overflow here.
    uint64_t user_ns = (__uint128_t)usage.ri_user_time * timebase.numer / timebase.denom;
    uint64_t system_ns = (__uint128_t)usage.ri_system_time * timebase.numer / timebase.denom;
    printf("{\"user_ns\":%" PRIu64 ",\"system_ns\":%" PRIu64
           ",\"rss_bytes\":%" PRIu64 ",\"footprint_bytes\":%" PRIu64
           ",\"idle_wakeups\":%" PRIu64 ",\"interrupt_wakeups\":%" PRIu64
           ",\"pageins\":%" PRIu64 ",\"read_bytes\":%" PRIu64 ",\"written_bytes\":%" PRIu64
           ",\"billed_energy_raw\":%" PRIu64 ",\"serviced_energy_raw\":%" PRIu64
           ",\"instructions\":%" PRIu64 ",\"cycles\":%" PRIu64
           ",\"user_ticks\":%" PRIu64 ",\"system_ticks\":%" PRIu64
           ",\"timebase_numer\":%u,\"timebase_denom\":%u",
           user_ns, system_ns, usage.ri_resident_size, usage.ri_phys_footprint,
           usage.ri_pkg_idle_wkups, usage.ri_interrupt_wkups, usage.ri_pageins,
           usage.ri_diskio_bytesread, usage.ri_diskio_byteswritten,
           usage.ri_billed_energy, usage.ri_serviced_energy, usage.ri_instructions, usage.ri_cycles,
           usage.ri_user_time, usage.ri_system_time, timebase.numer, timebase.denom);
    if (collected.flavor == 6) {
        printf(",\"rusage_flavor\":6,\"rusage_v6_sdk_available\":true,\"rusage_v6_errno\":null"
               ",\"rusage_fallback_reason\":null,\"energy_nj\":%" PRIu64 ",\"penergy_nj\":%" PRIu64,
               collected.energy_nj, collected.penergy_nj);
    } else {
        printf(",\"rusage_flavor\":4,\"rusage_v6_sdk_available\":%s,\"rusage_v6_errno\":",
               collected.v6_sdk_available ? "true" : "false");
        if (collected.v6_sdk_available) printf("%d", collected.v6_errno);
        else printf("null");
        printf(",\"rusage_fallback_reason\":\"%s\",\"energy_nj\":null,\"penergy_nj\":null",
               collected.v6_sdk_available ? "v6_einval" : "sdk_without_v6");
    }
    puts("}");
    return 0;
}
