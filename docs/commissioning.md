# Commissioning the installed Pi

Do these stages in order. Discovery is complete. USB-A and the first rear USB-C
port failed voltage checks. Rear USB4 passed startup, CPU load and one S3 cycle,
but the Pi restarted uncleanly during hibernate/resume. Reset investigation is
completed with a second failure while the gadget and daemon were stopped.
The user now prioritizes S3 sleep and accepts delayed Pi boot recovery for
hibernate/power-on. Keep the same rear USB4 port, no additional hardware and
no Windows software/settings changes; the front USB-C port is excluded.

Measured on 2026-11-01: Pi 4 Model B Rev 1.1, Ubuntu 26.04.1 LTS,
kernel `7.0.0-1017-raspi`, UDC `fe980000.usb`, connected DRM connector
`card1-HDMI-A-1`. Original EDIDs report Pi PA **1.1.5.0** and PC PA **1.1.1.0**.
Windows identifies MSI **MEG X870E ACE MAX (MS-7E85)**, BIOS **1.A30**, with
S3 sleep, hibernate and Fast Startup. Exact Onkyo model remains unverified.
Evidence is in the local ignored `evidence/` directory. Pi boot configuration,
the minimal gadget, power logger and live daemon are installed with backups.
Sleep/wake control was physically confirmed on 2026-11-02. Current kernel:
`7.0.0-1020-raspi`. The steps below document commissioning in order; see
[validation status](validation.md) for the latest passed and pending tests.

The user subsequently rebooted into kernel `7.0.0-1020-raspi`. The receiver
switched to the Pi's input during startup with the daemon still uninstalled.
This must be resolved independently of PC proxy routing. The staged Rust helper
can prepare a backed-up, removable boot setting:

```sh
sudo ~/pi-cec-stage/pi-cec prepare-boot --quiet-cec
```

It adds `hdmi_ignore_cec_init=1` and the second HDMI port equivalent, preserving
native Linux CEC. Raspberry Pi documents this
[firmware startup suppression option](https://www.raspberrypi.com/documentation/computers/legacy_config_txt.html#hdmi_ignore_cec_init)
as legacy; its presence in config.txt does not prove it works with this firmware.
Observe receiver input during a subsequent reboot with another source selected.
If it still switches, distinguish CEC announcements from receiver behavior on
HDMI signal appearance before choosing another remedy. The first observed reboot
after suppression preserved the receiver input, as confirmed by the user. Both
port-specific settings appear in the effective firmware configuration; repeated
cycle testing is still pending.

## 1. Discover, then test power first

The current development host reports Ubuntu 26.04.1 LTS, x86_64, with WSL kernel
`6.18.54.1-microsoft-standard-WSL2`. It exposes no Pi UDC or CEC device.
SSH administration and Windows interop work. The installed firmware uses Ubuntu's
`current/` / `new/` boot assets: the helper targets `current/cmdline.txt` and
refuses edits while a `new/` boot is pending. It preserves the existing dwc2
configuration when a peripheral controller is already available.

Use Ethernet/Wi-Fi SSH for administration. The tested ARM64 executable and
original/proxy EDIDs are already staged in `~/pi-cec-stage/` on
this Pi. The executable passed read-only native execution checks. Once APT
finishes and the power cable is changed, install only its power logger first:

```sh
cd ~/pi-cec-stage
mkdir -p evidence
sudo ./pi-cec inspect > evidence/pi-discovery.txt 2>&1
sudo ./pi-cec install
journalctl -u pi-cec-power -f
```

The current Ubuntu update stages kernel `7.0.0-1020-raspi` and reports that the
next startup boots twice. Allow the update's planned restart to complete before
starting a new power-continuity test; record the resulting kernel and boot ID.

Discovery includes OS, kernel, board model, boot command line/config, kernel
options, UDCs, DRM connectors, CEC adapters, modules, boot ID, uptime, firmware
voltage indicator and kernel warnings. Review included boot files and any existing
gadget services manually before modifying them. Install `v4l-utils` for `cec-ctl`.
If `vcgencmd` is absent, install the installed distribution's firmware utility or
use an instrumented voltage test; absent voltage evidence cannot pass this stage.

The original PC USB-A → Pi USB-C setup already failed the voltage test. For
the next test, use only the proposed PC USB-C → Pi USB-C data/power connection.
Its connector type alone does not establish adequate standby current. Record
timestamps for: extended awake idle, CPU load, repeated PC sleep/wake, shutdown,
reboot and supported hibernation. Allow at least several minutes in each off
state, plus an overnight idle run. Check voltage under load with an inline meter
if available. Compare firmware current and historical undervoltage indicators,
continuous uptime, boot ID, kernel logs, and the persistent samples:

```sh
sudo pi-cec power-report /var/lib/pi-cec/power.log
journalctl -u pi-cec-power --no-pager
journalctl -k --no-pager
```

The report identifies reboot IDs, missing indicators, sampling gaps and
undervoltage samples. It does not auto-certify phases that were never tested.
Gap detection uses uptime within each boot, so NTP clock corrections do not
look like lost power. If firmware flags are inaccessible, samples fall back
to the kernel's `rpi_volt` hwmon alarm. A reported alarm establishes failure;
clear sampled alarms alone cannot rule out brief events between samples.
Preserve the report before deliberately rebooting the Pi; separate test runs so
an intentional reboot is not mistaken for a power failure. Power logging begins
automatically after installation and across subsequent Pi boots.

The first rear USB-C trial also recorded undervoltage. The helper now offers a
reversible CPU limit for this lightweight appliance:

```sh
sudo pi-cec prepare-boot --low-power
```

This caps CPU clocks at 600 MHz and disables the initial turbo period, using
[documented firmware settings](https://www.raspberrypi.com/documentation/computers/config_txt.html#overclocking-options).
It does not disable voltage detection, change voltage settings, or mark power
verified. Reboot and repeat startup, load and standby-power tests. If undervoltage
persists, the port/cable still fails regardless of this setting.

[MSI documents](https://www.msi.com/support/technical_details/MB_BIOS_Sleep_Hibernate)
ErP Ready disabled and Resume By USB Device enabled for standby USB-A power.
Confirm the exact X870E board and BIOS before changing anything, and retain the
working Bluetooth controller wake configuration. These settings do not establish
available current. Raspberry Pi recommends a
[3 A supply for Pi 4](https://www.raspberrypi.com/documentation/computers/raspberry-pi.html).
The gadget's 500 mA descriptor cannot grant that current.

If the Pi resets, loses power or reports undervoltage, record the affected phase
as FAILED. With `power_scope=continuous`, this blocks bus-powered commissioning.
The user explicitly selected `power_scope=sleep`: require uninterrupted,
voltage-clean S3 sleep/wake and accept that S4/cold start can reboot the Pi.
Keep those failures visible; they are not continuous-power passes. Fresh USB
enumeration after Pi boot triggers the startup route check when
`startup_select_pc=true`. Repeated unexpected power loss and storage robustness
remain separate limitations; startup reconciliation does not prevent them.

For deployments requiring continuous power, a concrete alternative is an
appropriately rated PoE/PoE+ HAT supplying the Pi independently, together with a
USB adapter/cable that **disconnects PC VBUS** while passing D+, D− and ground.
Verify isolation and absence of PC backfeed before connection. A typical “USB
data blocker” does the opposite and is unsuitable. Set
`GADGET_SELF_POWERED=1` in `/etc/pi-cec/gadget.env` for this wiring and repeat USB
bus tests. Never directly parallel PC USB 5 V with an independent Pi supply.

## 2. Enable only the minimal USB gadget

After power passes, inspect boot settings and prepare the peripheral overlay:

```sh
sudo pi-cec prepare-boot --usb
sudo reboot
sudo pi-cec gadget start
```

This appends a marked `[all]` stanza with
`dtoverlay=dwc2,dr_mode=peripheral`. Existing dwc2/OTG settings or legacy gadgets
cause refusal for manual reconciliation, not silent replacement. Original files
get `.pi-cec-original` backups. The helper recognizes common `/boot/firmware` and
`/boot` layouts; other distro layouts require inspection first.

The gadget has one CDC ACM function, a stable board-specific serial number, and
class/subclass 02/02 for Windows' built-in usbser driver. Development VID/PID are
Linux gadget IDs `1d6b:0104`; obtain an assigned identity before distributing a
product. It has no HID, networking, mass storage, remote-wake descriptor, tty
getty, serial traffic reader or DTR dependency.

Windows can list exact device instances with this one-time PowerShell command:

```powershell
Get-PnpDevice -PresentOnly -Class Ports | Format-List FriendlyName,InstanceId
Get-CimInstance Win32_BaseBoard | Format-List Manufacturer,Product,Version
powercfg /a
powercfg /devicequery wake_armed
```

Usbser selective suspend is
[disabled by default](https://learn.microsoft.com/en-us/windows-hardware/drivers/usbcon/usb-driver-installation-based-on-compatible-ids).
Read-only inspection found no `IdleUsbSelectiveSuspendPolicy` property for this
gadget. No Windows settings or software were changed. The user requires operation
without PC-side installation or OS-specific configuration: do not change registry,
root-hub, Bluetooth or global USB settings. Validate extended awake idle and real
sleep/hibernate/shutdown on the actual host. If a host generates indistinguishable
USB events for awake idle and sleep, this bus-only design cannot meet the required
mapping on that host without changing the constraints.

## 3. Measure the installed kernel's actual events

First inspect `/sys/class/udc/*/state` through real sleep/resume and extended idle.
Use `usb_backend=sysfs` only if it demonstrably distinguishes these states. The
sysfs backend remains UNKNOWN until `usb_verified=true` is set.

If sysfs stays `configured` while suspended, use the default trace bridge:

```sh
sudo pi-cec trace start
sudo cat /sys/kernel/tracing/instances/pi-cec/trace_pipe
```

Stop that `cat` before starting the daemon: trace_pipe has one consumer. The
bridge uses the installed kernel's libcomposite suspend/resume/disconnect/reset
callbacks and gadget state tracepoint. It is implemented in Rust using
[tracefs kprobe events](https://docs.kernel.org/trace/kprobetrace.html), with no
private structure offsets or custom kernel module. It requires exactly one UDC
and refuses missing symbols/tracepoints. Kprobes are kernel-specific: compare
callback behavior with the actual installed dwc2/libcomposite kernel source and
record events before treating them as evidence. A new kernel invalidates the
configured `verified_kernel` and blocks live mode.

The gadget helper arms tracing before binding; the daemon preserves an already
armed trace buffer so it can read initial enumeration and subsequent suspend
events. Trace mode latches suspend even when sysfs remains configured. At startup a
configured snapshot alone remains UNKNOWN until an observed configuration or
resume event. This avoids assuming an already-sleeping PC is awake. After daemon
restart while the host is already configured, one real bus transition can be
required before control starts. Buffer loss, failed reads, missing UDC and CEC
event loss invalidate observation; a trace read failure restarts the process so
an old blocked reader cannot steal events from a replacement reader.

Test Windows awake idle for hours with the COM port closed and no data traffic.
If it suspends under these conditions, the requirement is not met. No USB-only
implementation can tell user/controller wake from a background Windows wake if
both resume/configure this same USB bus. Debounce filters short events; it cannot
recover that missing information. Modern Standby that keeps the bus active is
also indistinguishable from ordinary awake idle. Record either limitation as a
failed/inapplicable power-state test rather than using traffic timeouts.

After successful tests for the selected `power_scope`, set `power_verified=true`, `usb_verified=true`, and
`verified_kernel` to the Pi's exact `uname -r` in `/etc/pi-cec/pi-cec.conf`.

## 4. Discover both HDMI paths and build the proxy

The reported receiver model is “Onkyo zx50”; its exact model is still unverified.
Record the physical label, Pi input, RTX input, receiver output and LG input.
[TX-RZ50 documentation](https://support.onkyousa.com/hc/en-us/articles/9450440686356-TX-RZ50-Connecting-Playback-Devices)
is applicable only after confirming that model. Preserve existing successful
CEC settings and the complete RTX → receiver → LG 4K/120 Hz/HDR path.

Use `cec-ctl --list-devices`, adapter details, and connected DRM status to map the
Pi's actual connector. Never assume `/dev/cec0` or HDMI-A-1. Save the Pi's original
connector EDID before any override. On Pi, for the **discovered** connector:

```sh
cat /sys/class/drm/card1-HDMI-A-1/edid > evidence/pi-original.bin
pi-cec edid evidence/pi-original.bin
```

The path above is illustrative; replace it with discovery output. The PC-port PA
must come from that port's EDID, not its printed HDMI input number. Export the
active LG/Onkyo monitor's `EDID` binary from the exact Windows display instance's
`HKLM\SYSTEM\CurrentControlSet\Enum\DISPLAY\...\Device Parameters` key. Correlate
active monitor instances with `Get-CimInstance -Namespace root\wmi WmiMonitorID`.
Alternatively, during commissioning temporarily connect the Pi to the PC's
receiver input, capture that EDID, then restore both cables to their original
ports. This is a one-time physical measurement, not the normal video path.

```sh
pi-cec edid evidence/pi-original.bin evidence/pc-original.bin evidence/proxy.bin
```

The Rust tool validates all EDID blocks/checksums, the HDMI VSDB, and distinct
sibling physical addresses. It changes only the Pi EDID's HDMI PA and affected
checksum. Set `pi_pa` to the original Pi address and `pc_pa` to the measured PC
address, as four hexadecimal digits without dots. Bind `adapter` to the
discovered adapter path if more than one adapter is connected.

The PC proxy deliberately has the PC branch PA, although it is physically wired
to the spare Pi input. Apply that identity to the **Pi's DRM connector only**:

```sh
sudo pi-cec prepare-boot --proxy-edid evidence/proxy.bin --connector HDMI-A-1
sudo reboot
cec-ctl --list-devices
cec-ctl -d /dev/cecX
```

Replace both example connector/adapter names. This uses full KMS, a
`drm.edid_firmware=CONNECTOR:pi-cec/proxy.bin` boot parameter, and the installed
initramfs generator's firmware inclusion mechanism. This Ubuntu uses dracut:
the installer writes its documented
[`install_items` configuration](https://dracut-ng.github.io/dracut/man/dracut.conf.5.html).
The Pi connector also gets `video=CONNECTOR:1280x720@60D`, using the kernel's
[forced digital connector mode](https://docs.kernel.org/fb/modedb.html). This
keeps the saved EDID available when the receiver withholds EDID or hotplug on
its inactive Pi input, and gives the Pi a modest display mode. These settings
affect only the Pi; they do not change the RTX/receiver/TV video path. Verify
CEC PA readback with the receiver off, on another input, and after reconnection.
Older initramfs-tools systems use a small hook instead. Ubuntu's `current/`
boot layout is supported; pending `new/` boot assets must first complete a
successful boot before editing the active command line. Unsupported boot layouts,
missing full KMS and conflicting EDID overrides cause refusal. The DRM/CEC
driver must actually report `pc_pa` after boot, with receiver on and off and
after reconnection. If the override does not affect this kernel's CEC notifier,
proxy routing is **not available through this backend**; do not enable routing
verification. A separate CEC adapter whose physical address is user-configurable
is an alternative, subject to the same video-path and routing tests.

No code attempts `CEC_ADAP_S_PHYS_ADDR` on an EDID-managed adapter. The kernel
[only permits setting PA when the adapter advertises that capability](https://docs.kernel.org/userspace-api/media/cec/cec-ioc-adap-g-phys-addr.html).
With the verified firmware override, the kernel's automatic Report Physical
Address at address claim and its query responses use the same PC PA as Active
Source. Each application TX rechecks that the adapter still reports that PA.
The receiver therefore sees the PC route rather than the spare Pi route. This
is a deliberate proxy arrangement requiring actual receiver/TV validation;
sending Active Source successfully is not proof that the receiver obeyed it.

The default logical identity is a processor/specific-use address to avoid
consuming the three playback slots potentially used by PS5, Apple TV and Switch.
If this receiver requires a playback identity, establish a genuinely available
slot before setting `playback_identity=true`. The daemon polls for its own
address, maintains exclusive initiator/follower ownership, never falls back to
unregistered transmission, and does not evict existing devices. Queries about
physical address/version/name are handled consistently by the native kernel
framework. The proxy's own reported power is the always-running Pi's power;
observed TV/AVR power and PC USB state are tracked separately.

## 5. Commission routing and coexistence

Stop the dry-run service before claiming a logical address:

```sh
sudo systemctl stop pi-cec.service
sudo pi-cec --commission
```

This permits actual wake/routing tests while `routing_verified=false`, but only
after verified power/USB and correct PC proxy PA. Standby is forcibly disabled.
Cycle the host USB bus through real PC sleep/wake. The daemon sends TV Image View
On, receiver discrete Power On Function and release, waits the startup interval,
then announces the PC proxy as Active Source. It asks both devices for power
status. Whole wake sequences retry with bounded exponential backoff; another
source taking over cancels retries for that PC session.

Observe the actual Onkyo selected input, LG input and PC video/audio. Verify
4K/120 Hz/HDR from Windows with the original GPU cable path, both initially-off
and already-on devices, and all other sources taking over. CEC RX routing
information from the TV/receiver is evidence; transmitted messages and echoes
are not. Set Stream Path is treated as a request, not proof. A parent receiver
branch alone is unknown ownership. Physical verification remains mandatory even
if both power reports and a switch routing report arrive.

Three explicit standby policies are available:

* `fresh` (default): after sustained inactivity, request current source and require
  fresh positive TV/receiver PC-routing evidence in the query window. Otherwise
  log and skip standby. Standard CEC has no universally supported selected-input
  query, and Request Active Source may produce no switch-routing response. This
  policy can therefore skip standby even when the PC still owns the screen.
* `tracked`: retain previously observed positive PC routing through an
  uninterrupted CEC observation session, while requesting source status before
  standby to detect takeovers. Unknown state, adapter/event loss, reconnection,
  restart and a different source invalidate that ownership. Use only after
  testing every console and receiver input selection produces observable CEC
  takeover traffic. If silent manual/receiver changes occur, use `fresh`.
* `session`: standard CEC source-claim tracking, selected for this installation.
  A completed successful PC Active Source transmission establishes `PcClaimed`,
  distinctly from observed `Pc` routing. The daemon asks for active sources
  before standby and yields to any other source. Adapter loss, event loss and
  process restart clear ownership. Actual routing is physically validated during
  commissioning; a held claim is never logged as measured input selection.
  Silent input changes are not observable through this policy.

The `fresh` and `tracked` policies require observed routing. Another source's
Active Source, trusted receiver/TV Routing Change/Information, and TV stream
selection update ownership. Standby is directed to the receiver and TV rather
than broadcast. A bus reset shorter than the off debounce cancels pending
standby without reasserting the PC input. Device loss does not initiate standby.
On startup inactive USB never triggers standby. With the default
`startup_select_pc=false`, existing other-source ownership is respected.
With the user's selected `startup_select_pc=true`, fresh active enumeration
triggers a bounded source/power query. If the AV system reports the PC route and
both devices on, no selection is sent. Otherwise the daemon selects the PC once
with bounded retries. Responses identifying an existing source during that
initial query are baseline observations; subsequent takeovers cancel retries.
Already-on devices do not incur the startup warmup delay. A service restart
without fresh USB events remains conservative and may await the next transition.

This LG answered Request Active Source with `0.0.0.0` during the PC session,
including after PC sleep, causing the initial standby test to be skipped.
The user specified that the LG always uses the receiver input, so this installation
sets `tv_always_receiver=true`: TV root Active Source replies do not displace
the held receiver-source claim. Console Active Source messages, receiver routing
changes, and TV Set Stream Path requests still invalidate it. TV-native apps are
outside the protection offered by this fixed-input setting. The default remains
false for installations that use TV-native sources.

A one-time Onkyo network input query was explored during diagnosis. The network
implementation was removed before deployment at the user's request; runtime
uses only HDMI-CEC and USB, with systemd restricting address families to AF_UNIX.

After all applicable tests, set `routing_verified=true`, `standby=true`, and the
tested `standby_policy`, then enable normal service:

```sh
sudo pi-cec --check
sudo pi-cec enable-live
journalctl -u pi-cec -f
```

Normal operation needs no WSL process or Windows task. systemd starts the gadget
and daemon on Pi boot, restarts failures with backoff, and bounds repeated failed
starts. Missing adapters are rediscovered periodically. Pending wake work expires
while an adapter is absent rather than stealing the input much later. A CEC
reconnection invalidates ownership; it does not continuously enforce the PC input.

## Uninstall and recovery

Before uninstalling, remove managed boot settings and stop the service:

```sh
sudo systemctl stop pi-cec.service
sudo pi-cec prepare-boot --remove
sudo pi-cec uninstall
sudo reboot
```

Installation is idempotent and preserves preexisting configuration. Changed
files have adjacent `.pi-cec-original` backups or `.pi-cec-absent` markers. Boot
removal strips only the managed block/override; other subsequent boot edits are
retained. Uninstall restores original installed files and saves the current
versions as `.pi-cec-uninstalled`. Power logs and backups are retained. Keep the
build's `target/release/pi-cec` available for recovery if the installed binary is
removed. No Windows configuration needs restoring.
