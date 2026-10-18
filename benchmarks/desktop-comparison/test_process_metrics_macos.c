// Synthetic fallback fixture: never calls real libproc or native APIs.
#include <libproc.h>
#include <mach/mach_time.h>
#include <sys/resource.h>
#include <assert.h>
#include <errno.h>
#include <string.h>
#if defined(TEST_SDK_WITHOUT_V6)
#undef RUSAGE_INFO_V6
#endif

enum scenario { V6_OK, V6_UNSUPPORTED, V6_DENIED, V6_MISSING, V4_DENIED };
static enum scenario selected;
static int v4_calls, v6_calls;
static struct rusage_info_v4 expected_common(void) {
    struct rusage_info_v4 v={0};
    v.ri_user_time=111;v.ri_system_time=222;v.ri_resident_size=333;v.ri_phys_footprint=444;
    v.ri_pkg_idle_wkups=5;v.ri_interrupt_wkups=6;v.ri_pageins=7;
    v.ri_diskio_bytesread=8;v.ri_diskio_byteswritten=9;v.ri_billed_energy=10;
    v.ri_serviced_energy=11;v.ri_instructions=12;v.ri_cycles=13;return v;
}
static int fake_proc_pid_rusage(int pid,int flavor,rusage_info_t *buffer) {
    assert(pid==42);
    if(flavor==RUSAGE_INFO_V4) {
        v4_calls++;if(selected==V4_DENIED){errno=EACCES;return -1;}
        struct rusage_info_v4 v=expected_common();memcpy(buffer,&v,sizeof(v));return 0;
    }
#if defined(RUSAGE_INFO_V6)
    assert(flavor==RUSAGE_INFO_V6);v6_calls++;
    if(selected==V6_UNSUPPORTED||selected==V4_DENIED){errno=EINVAL;return -1;}
    if(selected==V6_DENIED){errno=EPERM;return -1;}
    if(selected==V6_MISSING){errno=ESRCH;return -1;}
    struct rusage_info_v6 v={0};struct rusage_info_v4 common=expected_common();
    memcpy(&v,&common,sizeof(common));v.ri_energy_nj=1234;v.ri_penergy_nj=567;
    memcpy(buffer,&v,sizeof(v));return 0;
#else
    assert(0 && "old-SDK branch requested V6");return -1;
#endif
}
#define proc_pid_rusage fake_proc_pid_rusage
#define main collector_main
#include "process_metrics_macos.c"
#undef main
#undef proc_pid_rusage
static void reset(enum scenario s){selected=s;v4_calls=v6_calls=0;errno=0;}
static void check_common(const struct collector_usage *r){struct rusage_info_v4 v=expected_common();assert(memcmp(&r->common,&v,sizeof(v))==0);}
int main(void) {
    struct collector_usage r;
#if defined(RUSAGE_INFO_V6)
    reset(V6_OK);assert(read_process_usage(42,&r)==0);check_common(&r);
    assert(v6_calls==1&&v4_calls==0&&r.flavor==6&&r.v6_sdk_available==1);
    assert(r.v6_errno==0&&r.energy_nj==1234&&r.penergy_nj==567);
    reset(V6_UNSUPPORTED);assert(read_process_usage(42,&r)==0);check_common(&r);
    assert(v6_calls==1&&v4_calls==1&&r.flavor==4&&r.v6_errno==EINVAL);
    assert(r.energy_nj==0&&r.penergy_nj==0); // Output protocol supplies null for unavailable.
    reset(V6_DENIED);assert(read_process_usage(42,&r)==-1&&errno==EPERM);assert(v6_calls==1&&v4_calls==0);
    reset(V6_MISSING);assert(read_process_usage(42,&r)==-1&&errno==ESRCH);assert(v6_calls==1&&v4_calls==0);
    reset(V4_DENIED);assert(read_process_usage(42,&r)==-1&&errno==EACCES);assert(v6_calls==1&&v4_calls==1);
#else
    reset(V6_OK);assert(read_process_usage(42,&r)==0);check_common(&r);
    assert(v6_calls==0&&v4_calls==1&&r.flavor==4&&r.v6_sdk_available==0);
    assert(r.v6_errno==0&&r.energy_nj==0&&r.penergy_nj==0);
    reset(V4_DENIED);assert(read_process_usage(42,&r)==-1&&errno==EACCES);assert(v6_calls==0&&v4_calls==1);
#endif
    return 0;
}
