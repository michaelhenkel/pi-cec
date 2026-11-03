# Validation status

SSH discovery of the Pi succeeded on 2026-11-01. Raw evidence is retained locally
in the ignored `evidence/` directory and is not included in this repository.
The main S3 sleep/wake flow is installed and physically confirmed working by
the user on 2026-11-02. Console coexistence and extended validation remain pending.
Both the original
USB-A setup and the first rear USB-C test produced undervoltage. The Pi now runs
power logging and the live daemon with CEC session ownership. A 600 MHz CPU cap and Pi-only
EDID/720p connector override are installed. The original rear USB-C port still
logged undervoltage with the CPU limit. The rear USB4/40 Gbps port passed its
first boot, CPU load and one S3 cycle with no undervoltage, but failed continuity
during S4 hibernate/resume. The setup retains the same rear USB4 port;
the user declines using the front port. The user subsequently chose to prioritize
S3 sleep and accept delayed Pi boot recovery for hibernate and power-on. The user
requires no additional hardware and no PC-side software or settings changes.

## Passed

* **Physical sleep/wake result, 2026-11-02:** the user confirmed that sleep turns
  off both receiver and TV, and the Xbox controller button wakes everything.
  Previously the user confirmed PC picture on the correct RTX receiver input.
  Services are enabled and active, installed/release binary hashes match,
  and current firmware voltage flags are `0x0` on boot
  `0be167dd-be92-493b-ad25-c1cf6e86da72`. Journal evidence shows TV-first standby
  and wake continuing through receiver HDMI/CEC resets. This pass includes
  physical observation, not just transmission success.
  All 368 power samples captured from this boot had zero firmware flags;
  maximum sample gap was 2.23 seconds. Its startup journal also records fresh
  USB enumeration, route reconciliation, and successful wake recovery.
  Evidence: `evidence/sleep-working-final.txt`,
  `evidence/sleep-working-transitions.txt`, `evidence/sleep-working-power.log`.
* Final source structure checks: valid Cargo manifest, all embedded assets
  present, consistent service executable references, no Python/PowerShell/shell
  script files. All **50 Rust tests passed natively on the Pi**; 40 earlier tests
  passed on WSL before the latest boot-management changes.
* Release builds passed for Linux x86_64 and `aarch64-unknown-linux-gnu`.
  The current source also built in release mode natively on the Pi.
* ARM64 executable staged at `~/pi-cec-stage/pi-cec` on the Pi;
  its SHA-256 matched the WSL build. `--help` and a read-only EDID command
  executed successfully, confirming the Pi connector PA `1150`.
* Installed systemd units passed verification on Pi (unrelated distribution XFS
  units emit a deprecated CPUAccounting warning). Power logging and dry-run
  daemon services ran successfully. Live CEC session mode is now enabled for
  the user's sleep-focused commissioning.
* Pi discovery: Raspberry Pi 4 Model B Rev 1.1; Ubuntu 26.04.1 LTS;
  kernel `7.0.0-1017-raspi`; two CEC adapters and UDC `fe980000.usb` present.
  After the user rebooted, kernel `7.0.0-1020-raspi` booted successfully and
  `dpkg --audit` reported no unfinished package operations.
* Original HDMI EDIDs captured: Pi **1.1.5.0**, PC **1.1.1.0**. Rust proxy
  generation passed. The installed override now makes native `/dev/cec0` report
  the correct PC PA `1.1.1.0` while the Pi input is inactive. Actual routing and
  wake from standby subsequently passed; logical-address query consistency and
  physical cable reconnect tests remain pending.
* Gadget enumerates as USB Serial Device (COM3), Microsoft `usbser`, driver
  `10.0.26100.9818`, hardware ID `USB\\VID_1D6B&PID_0104`.
  No COM port was opened. No explicit IdleUsbSelectiveSuspendPolicy is present.
* Real gadget unbind/rebind generated inactive/active transitions on kernel
  `7.0.0-1020-raspi`. The daemon detected both and debounced activation.
* After CEC startup suppression, the user confirmed that booting the Pi did
  **not** switch the Onkyo input. Firmware reports both port-specific suppression
  settings as enabled. Repeat-cycle testing is still pending.
* First Windows sleep/wake diagnostic cycle passed Pi power continuity and USB
  event detection. Bus inactive at 18:48:28 PST, active at 18:49:21 (53.08 s).
  The same Pi boot ID persisted; 89 power samples had a maximum 2.10 s gap and
  no new undervoltage entries. The user confirmed return after the controller-wake
  instruction. Direct sysfs sampling missed the inactive interval; actual
  sysfs suspend behavior and independent wake-source identification remain pending.
  Evidence: `evidence/sleep-observer-result.txt` and accompanying raw logs.
* Windows inventory: MSI MEG X870E ACE MAX (MS-7E85), BIOS 1.A30;
  S3/hibernate/Fast Startup available. No Windows settings changed.
* Rear USB4/40 Gbps port: boot `7dcf261c-6452-4e00-91dd-8e5a189c6a6c`
  remained at firmware flags `0x0` through startup, a four-process SHA-256
  benchmark bounded to 60 seconds, and a native release build. The 600 MHz
  CPU limit remains installed. No kernel undervoltage events were recorded.
  Evidence: `evidence/pi-usb4-load.txt`, `evidence/pi-usb-trace-build.txt`.
  This load check does not validate standby power or extended idle.
* Rear USB4/40 Gbps S3 cycle: the same boot ID persisted and firmware flags
  remained `0x0`. Actual `composite_suspend` at Pi uptime 487.943323 s and
  `composite_resume` at 535.825765 s were recorded. Sysfs was **configured at
  both events**, demonstrating that polling that file alone cannot detect sleep
  on this kernel. The event bridge detected inactive/active correctly and
  triggered a debounced dry-run wake sequence. Windows logged sleep reason
  Application API, its suspend call returned true, and the user confirmed return.
  Windows lists USB/xHCI and USB4 controllers as wake sources, which does not
  independently identify the Xbox controller. Evidence:
  `evidence/usb4-coordinator-before.txt`, `evidence/usb4-coordinator-after.txt`,
  `evidence/usb4-windows-wake.txt`.
* Workspace and development-host discovery completed: WSL Ubuntu 26.04.1 LTS,
  kernel `6.18.54.1-microsoft-standard-WSL2`, no exposed Pi UDC/CEC adapter.
* The earlier Python EDID prototype passed seven tests before being removed at
  the user's request. Those results **do not validate the replacement Rust
  implementation**; its equivalent tests are now part of Cargo's test suite.

## Failed hardware check

The user initially confirmed PC USB-A supplied the Pi. Boot
`295dd3c8-1d0c-4d75-a9e5-953fe44aed49` logged undervoltage at monotonic
9.27, 11.30, 31.80, 238.63, 531.49, 568.36 and 625.70 seconds;
the latest captured event cleared at 629.79 seconds.
Evidence: `evidence/pi-power-boot.txt`, `evidence/pi-power-followup.txt`,
and `evidence/pi-power-before-cable-change.txt`.
This establishes inadequate voltage in the original USB-A port/cable setup,
not whether the port or cable is the cause. USB-A sleep/shutdown continuity
was not tested.

Boot `4ce12f16-bd81-4845-8dde-4e44d2592cb4` on the updated kernel also logged
undervoltage at 11.94 and 20.13 seconds, clearing at 18.08 and 28.32 seconds.

After changing to a rear PC USB-C connection, boot
`15e28de4-db78-4abf-bf73-f619d6b62e18` logged undervoltage at 13.74, 19.88,
322.98 and 333.22 seconds. The first USB-C trial therefore also failed voltage
validation. Firmware historical flags were `0x50000`. The CPU was subsequently
  capped at 600 MHz; a fresh boot and load test were required.

The fresh boot `e7d72094-ee84-40f9-b042-e9dd2d47bcec` confirms the 600 MHz
limit and Pi-only EDID override are effective, but logged undervoltage from
29.04 to 35.17 seconds. CPU limiting therefore did **not** resolve power.
Evidence: `evidence/pi-low-power-proxy-boot.txt`.

Rear USB4 S4 hibernate/resume **failed continuity**. Boot
`7dcf261c-6452-4e00-91dd-8e5a189c6a6c` ended without a recorded clean shutdown;
the next boot is `f491167e-3d83-4395-8e45-4fb26be0c375`. Journald reports an
unclean prior shutdown and the firmware partition had its dirty bit set.
The gadget suspend callback at uptime 690.258401 s correctly caused
PC_ACTIVE -> PENDING_OFF; after 15 s it queried ownership, and after another
3 s skipped standby with unknown ownership (standby was also disabled).
The fsynced power log continued through uptime 741.26 s / 19:06:35 PST,
about 51 s after suspend. All 357 samples in that boot had flags `0x0`.
Windows recorded actual hibernation and resume at 19:07:03 PST. The new Pi
boot's uptime implies Linux started around 19:06:59 PST after correcting its
initial unset clock. The timing points toward a reset around PC resume;
the precise interruption time and physical cause are not established.
No persisted crash report was present, and the runtime watchdog is disabled.
Firmware `get_rsts` and the bootloader device-tree reset field both read
`0x1000`; that alone is not conclusive because firmware can reset the PMIC.
Evidence: `evidence/usb4-hibernate-power-failure.txt`,
`evidence/pi-power-after-usb4-hibernate.log`,
`evidence/usb4-hibernate-reset-diagnostics.txt`,
`evidence/usb4-windows-power-events.txt`, `evidence/usb4-reset-firmware.txt`.
The same rear port was retested with the USB gadget, CEC daemon and trace bridge
stopped. Power logging remained active. Continuity **failed again**: boot
`f491167e-3d83-4395-8e45-4fb26be0c375` ended with its last fsynced sample at
19:18:59 PST, uptime 720.90 s. All 347 samples had flags `0x0`. The next boot was
`abe3164e-29af-4b71-b9fc-8892d4f85561`. Windows recorded S4 resume completing at
19:19:27.947 PST. Both failures stopped logging approximately 28 seconds before
Windows finished resuming. The failure therefore reproduces without the gadget,
trace bridge or CEC daemon running; their operation is not required to cause it.
The evidence strongly suggests a power interruption during the PC wake sequence,
but is not an electrical measurement. Services recovered in dry-run after reboot.
Evidence: `evidence/usb4-unbound-before.txt`, `evidence/usb4-unbound-after.txt`,
`evidence/usb4-unbound-trace-stopped.txt`,
`evidence/pi-power-after-unbound-hibernate.log`,
`evidence/usb4-unbound-windows-resume.txt`.

The user confirmed ErP Ready is Disabled and Resume By USB Device is Enabled.
The user declines the front USB-C port; the existing rear USB4 connection stays
in place. No verification flags were enabled during these failure investigations.
The user subsequently accepted Pi reboot/delayed recovery for hibernate/power-on,
and selected S3 sleep as the primary supported path. `power_scope=sleep` records
this revised acceptance criterion; it does not reclassify the S4 failures as passes.
Startup reconciliation now queries the route/power after a fresh enumeration
and can select the PC, while initial inactive USB still never causes standby.
Three new Rust tests cover startup reconciliation, baseline versus takeover,
and cancellation by suspend. The subsequent session-ownership, fixed-receiver
TV and delayed-enumeration tests bring the total to 50 passing Rust tests.
Native release builds pass. Hardware startup/repeated-sleep tests remain pending.

## Awake-only CEC diagnostics

On boot `abe3164e-29af-4b71-b9fc-8892d4f85561`, stopped the dry-run daemon and
claimed specific-use LA 14 with v4l-utils, CEC 1.4, OSD `Windows PC`, no vendor ID,
and the already-installed kernel PA `1.1.1.0`. TV and AVR both returned power-on
status. TV identified itself as LG `OLED77G6WU`, LA 0, PA `0.0.0.0`; receiver
returned Onkyo vendor ID, OSD `AV Receiver`, LA 5, PA `1.1.0.0`. This confirms
CEC communication and the usable proxy identity, not physical input switching
or the exact Onkyo model. Evidence: `evidence/cec-tv-avr-probe.txt`.

The user then physically confirmed correct PC picture through the Onkyo RTX
input after the daemon announced PA `1.1.1.0`. Source capture before gadget
binding also passed: the daemon consumed the initial configuration event and
executed startup reconciliation. Evidence: `evidence/pi-startup-route-live.txt`.

The first live S3 standby cycle **failed AV standby**: the LG responded to
Request Active Source with `[0f,82,00,00]`, which displaced the PC claim and
caused conservative SkipStandby. USB suspend/resume and Pi continuity still
worked; voltage flags remained `0x0`. The fix adds explicit
`tv_always_receiver=true` for the user's fixed-input TV arrangement, keeping
console takeovers protected. Evidence: `evidence/live-sleep-after.txt`.

The next S3 cycle reached Standby and queued directed receiver/TV commands;
receiver link loss interrupted the sequence. On resume, a second HDMI/CEC loss
also cancelled pending wake work. The next fix sends TV standby first and
retains bounded wake work across adapter loss while clearing observed ownership.
USB resumes/other-source takeovers still cancel work through state/generation.
The user subsequently confirmed physical off/on operation with the corrected
sequence on 2026-11-02; see the passing result above.
Evidence: `evidence/live-sleep-retest-after.txt`,
`evidence/pi-hdmi-recovery-build.txt`.

Before suppression, Pi reboot selected the Onkyo's **Pi HDMI input**. That
specific observed behavior passed its first retest after suppression.

## Earlier development blockers (resolved)

* Rust/GCC/UAPI headers were initially absent. Temporary WSL toolchains enabled
  earlier builds. Current development tools and v4l-utils are now installed on
  the Pi; no new tools were installed on the PC for this commissioning step.
* Shell network access was initially blocked; it is now enabled.
* The earlier sandbox restriction on `systemd-analyze verify` is resolved.
  Power and dry-run unit execution now pass on the Pi.
* Windows interop and SSH work. The user enabled passwordless sudo on the Pi;
  no password was requested or stored by the agent.

Pi service and boot configuration changes have adjacent backups. No Windows
policy or Bluetooth controller-wake setting was changed. Live sleep operation
is enabled; the remaining tests below are not implied to pass by that result.

## Reproducing software checks

Run on a Linux host with a compiler and Linux UAPI headers:

```sh
cargo fmt
cargo test --all-targets
cargo build --release
sudo systemd-analyze verify systemd/*.service
```

The last command needs the binary installed at `/usr/local/bin/pi-cec`; on WSL,
verification used temporary copies of the units with that path changed to the
built executable, without installing or starting services.

Rust tests cover debounce, startup inactive, extended active idle, brief reset,
pending-off cancellation, confirmed/unknown/other ownership, both standby
policies, restart, cycles, source queries versus evidence, spoofed/self routing
messages, EDID integrity/physical addresses, and managed boot-block removal.
Boot preparation distinguishes dracut from initramfs-tools, including Ubuntu's
dracut-provided `update-initramfs` compatibility command.
Power-log tests cover reboot detection, missing samples, partial hwmon evidence,
and system-clock corrections without falsely reporting a power gap.
USB trace tests preserve a suspend event arriving in the same poll as a
configuration transition and keep startup snapshots conservative.
They do not require actual USB or CEC hardware. Native ioctl behavior requires
the Pi integration tests below.

## Pending hardware tests

Record timestamps, boot ID, firmware voltage flags, USB events, CEC TX/RX, actual
Onkyo selection, LG input and visible video/audio for each case. A successful TX
or ACK is insufficient.

| Test | Required result | Status |
|---|---|---|
| Installed OS/kernel/boot/devices | Actual Pi discovery recorded | Passed |
| Exact Onkyo and MSI models | Labels/device inventory confirmed; “zx50” unresolved | Pending |
| Sustained PC USB standby power | No undervoltage and continuous sleep/shutdown power | USB4 boot/load/S3 passed once; S4 continuity failed; S5 pending |
| Load and overnight voltage | No current/historical undervoltage, sample gaps or reboot | Pending |
| Minimal gadget enumeration | Built-in usbser, no open COM port required | Passed |
| Kernel USB suspend/resume | Actual bus events distinguish required host states | S3 trace passed; sysfs stays configured through suspend and is unsuitable alone |
| Extended Windows awake idle | No false inactive/suspend event | Pending |
| Xbox controller wake | Bluetooth wake remains working; PC AV route selected | Physically confirmed working by user |
| Sleep / shutdown | Debounced inactive; standby only for owned PC session | Sleep and physical AV standby/wake passed; shutdown recovery pending |
| Reboot | Brief USB reset cancels off; no unnecessary source reassertion | Pending |
| Hibernate / fast startup | Record actual bus behavior; mark unsupported if unavailable | S4 suspend detected; Pi reset during hibernate/resume; fast startup pending |
| Repeated sleep/wake | At least 20 cycles with no missed/crossed actions | Pending |
| Both physical addresses | Pi 1.1.5.0 and PC 1.1.1.0 from original EDIDs | Passed |
| Proxy identity/query consistency | Kernel PA and automatic/query Report PA equal PC PA | Pending |
| AV initially off | Receiver/TV wake; correct PC input, LG receiver input | User confirmed wake after both entered standby; correct PC input also confirmed |
| AV already on another input | PC transition selects correct actual port | PC route confirmed; deliberately different source case pending |
| PC 4K/120 Hz/HDR | Original RTX path/capabilities unchanged | Pending |
| PS5 / Apple TV / Switch takeover | Each takes control; PC retries/enforcement stop | Pending |
| PC sleeps after takeover | Other session remains powered and selected | Pending |
| Unknown/silent source ownership | Standby skipped with reason | Pending |
| USB reconnection | Measured bus recovery, no traffic dependency | Software unbind/rebind passed; physical reconnection pending |
| HDMI/receiver reconnection | Correct adapter/proxy identity recovers; ownership invalidated | Receiver power-transition resets recovered; physical cable reconnection pending |
| Daemon restart while active/inactive | Reconciliation, no blind standby or stale ownership | Pending |
| Pi reboot | systemd/gadget recovery; startup reconciliation | Observed on latest boot; full PC cold-start/shutdown matrix pending |
| Pi boot preserves another source | No selection of the Pi input during firmware/kernel startup | First retest passed after suppression |
| Idempotent install/uninstall | Original settings preserved/restored; backups retained | Pending |

Record USB-only ambiguity separately: background resume may be indistinguishable
from controller/user wake; Modern Standby may leave this bus active. If observed,
the hardware cannot satisfy that distinction under the stated constraints.
