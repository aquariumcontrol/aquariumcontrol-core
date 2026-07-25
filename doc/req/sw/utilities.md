### Process OS Shutdown Signals
`swreq~utilities_process_signals~1`

The signal handler shall capture operating system terminal signals (like SIGINT, SIGTERM, SIGABRT) and coordinate clean, sequential shutdown sequences across all running loops.

Covers:

* sysreq~utilities1~1
Needs: impl, utest

### Parse TOML Configuration
`swreq~utilities_parse_toml_config~1`

The application shall parse configuration parameters from a local TOML configuration file on startup to initialize device parameters and pins.

Covers:

* sysreq~utilities2~1
Needs: impl, utest

### Check Database Version Compatibility
`swreq~utilities_check_version_compatibility~1`

The application shall compare its compiled version information and hash against the database logging table schema on boot to prevent schema mismatches.

Covers:

* sysreq~utilities3~1
Needs: impl, utest

### Write PID File
`swreq~utilities_write_pid_file~1`

The application shall write its active process ID (PID) to a local file on startup, preventing duplicate instances from initiating.

Covers:

* sysreq~utilities4~1
Needs: impl, utest
