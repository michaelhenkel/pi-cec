# Pi CEC appliance

One Rust executable detects the Windows host's USB bus state and controls an AV
system through Linux's native CEC framework. It leaves the RTX → Onkyo → LG video
path intact. No Windows service, application, COM-port session, network heartbeat,
or controller-wake changes are required.

**Sleep/wake is working and enabled.** On 2026-11-02 the user confirmed that PC
sleep turns off both the receiver and TV, and the Xbox button wakes everything.
Correct RTX input selection and PC picture on the LG were also physically
confirmed. All 50 Rust tests passed. Runtime uses only USB and HDMI-CEC.
USB-A and the initial rear USB-C port produced undervoltage, including with
the 600 MHz CPU limit. The rear USB4/40 Gbps port passed boot, CPU load and one
S3 sleep cycle. Hibernate/resume restarted the Pi even with the gadget stopped.
The user now accepts delayed Pi boot recovery for hibernate/power-on and
prioritizes ordinary sleep. `power_scope=sleep` records that limited validation;
`startup_select_pc=true` checks AV routing after fresh USB enumeration at boot.
The daemon uses HDMI-only session ownership, the fixed-receiver LG setting,
TV-first standby, and wake recovery across receiver HDMI resets. Console
takeover, extended cycling and full cold-start validation remain pending. See
[commissioning](docs/commissioning.md) and [test status](docs/validation.md).

## Build

Build on Linux with Rust 1.74+, a C compiler, `ar`, and Linux CEC UAPI headers.
There are no Cargo dependencies. On the installed Pi's Ubuntu distribution:

```sh
sudo apt update
sudo apt install cargo rustc build-essential linux-libc-dev v4l-utils
cargo test --all-targets
cargo build --release
```

The distribution and package availability must first be checked on the actual Pi.
No command here reinstalls or reflashes its OS. Native builds on the Pi are the
supported deployment path; WSL can build/test the hardware-independent policy and
copy source over SSH. The build script's `CC` override is available, but a cross
toolchain/sysroot must be configured separately.

## Components

| File | Responsibility |
|---|---|
| `src/lib.rs` | Pure UNKNOWN / PC_INACTIVE / PC_ACTIVE / PENDING_OFF state machine |
| `src/main.rs` | CEC receive policy, bounded wake sequence, ownership and recovery |
| `src/usb.rs` | Validated sysfs backend or scoped libcomposite event bridge |
| `src/edid.rs` | Checked Pi-only physical-address EDID proxy preparation |
| `src/admin/` | Rust diagnostics, power logging, gadget/tracing, boot, install/uninstall |
| `native/cec.c` | Small C shim for Linux's CEC ioctl ABI |
| `systemd/` | Daemon, minimal CDC ACM gadget, and power validation services |

There are no Python, PowerShell, or shell script files. On this Pi's Ubuntu,
the optional firmware override uses a one-line dracut configuration. On older
initramfs-tools systems only, it generates a five-line shell hook required by
that tool's protocol. All substantive setup logic is Rust. The C shim avoids
duplicating kernel structure layout and ioctl-number calculations in Rust.

## Modes

```sh
target/release/pi-cec --help
sudo target/release/pi-cec inspect
sudo target/release/pi-cec install
sudo /usr/local/bin/pi-cec --diagnostic
sudo /usr/local/bin/pi-cec --commission
sudo /usr/local/bin/pi-cec --check
```

`install` enables only persistent power logging. `--diagnostic` and `--dry-run`
use monitor mode and claim no CEC logical address; intended TX is logged and
suppressed. `--commission` requires verified power, verified USB events and the
correct proxy PA, and always disables standby. Normal live mode additionally
requires routing verification and the exact tested kernel release.
Power verification is scoped explicitly: `continuous` requires all host power
states; `sleep` accepts reset/delayed recovery outside the validated sleep path.
The latter does not make unexpected power loss or storage reliability tested.

Logs go to journald with wall-clock timestamps and CEC TX/RX frames. Timer
decisions use monotonic time. A TX queue success or CEC ACK never sets observed
routing. `session` policy tracks a successful CEC active-source claim separately
from observed routing, yields to other sources, and loses ownership on adapter
or observation loss. The daemon does not continuously enforce the PC input.
There are no runtime network requests or receiver API dependencies.

Configure [config/pi-cec.conf](config/pi-cec.conf) after measuring the topology.
Never enable verification flags merely to bypass a missing hardware test.
