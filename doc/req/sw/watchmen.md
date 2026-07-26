### [watchmen] Self-Monitoring
`swreq~watchmen_self_monitoring~1`

The control system shall monitor thread loop execution times and track mutex locking delay durations, recording warning flags if delays exceed configurable thresholds.

Covers:

* sysreq~watchmen~1
Needs: impl, utest

### [watchmen] Pet Watchdog
`swreq~watchmen_pet_watchdog~1`

The watchdog loop shall write the heartbeat signal to the configured watchdog file path at regular intervals.

Covers:

* sysreq~watchmen2~1
Needs: impl, itest

### [watchmen] Trigger Reboot on Stall
`swreq~watchmen_reboot_on_stall~1`

The watchdog loop shall stop petting the watchdog and execute clean component shutdowns if critical errors or execution stalls are detected, triggering a safe system reboot.

Covers:

* sysreq~watchmen3~1
Needs: impl, utest
