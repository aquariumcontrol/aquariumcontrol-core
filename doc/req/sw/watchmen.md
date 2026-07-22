# Watchmen Software Requirements

### Self-Monitoring
`sw_req~watchmen_self_monitoring~1`

The control system shall monitor thread loop execution times and track mutex locking delay durations, recording warning flags if delays exceed configurable thresholds.

Covers: sys-req~watchmen~1
Needs: impl, utest

### Pet Watchdog
`sw_req~watchmen_pet_watchdog~1`

The watchdog loop shall write the heartbeat signal to the configured watchdog file path at regular intervals.

Covers: sys_req~watchmen2~1
Needs: impl, utest

### Trigger Reboot on Stall
`sw_req~watchmen_reboot_on_stall~1`

The watchdog loop shall stop petting the watchdog and execute clean component shutdowns if critical errors or execution stalls are detected, triggering a safe system reboot.

Covers: sys_req~watchmen3~1
Needs: impl, utest
