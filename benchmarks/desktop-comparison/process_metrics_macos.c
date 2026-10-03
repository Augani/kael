// Read-only per-process counters from the macOS SDK's versioned libproc ABI.
#include <libproc.h>
#include <mach/mach_time.h>
#include <sys/resource.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    struct rusage_info_v4 usage = {0};
    if (proc_pid_rusage(atoi(argv[1]), RUSAGE_INFO_V4, (rusage_info_t *)&usage) != 0) return 1;
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
           ",\"timebase_numer\":%u,\"timebase_denom\":%u}\n",
           user_ns, system_ns, usage.ri_resident_size, usage.ri_phys_footprint,
           usage.ri_pkg_idle_wkups, usage.ri_interrupt_wkups, usage.ri_pageins,
           usage.ri_diskio_bytesread, usage.ri_diskio_byteswritten,
           usage.ri_billed_energy, usage.ri_serviced_energy, usage.ri_instructions, usage.ri_cycles,
           usage.ri_user_time, usage.ri_system_time, timebase.numer, timebase.denom);
    return 0;
}
