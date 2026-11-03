/* Small native Linux CEC ABI boundary. No libCEC or subprocess per message. */
#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <fcntl.h>
#include <glob.h>
#include <linux/cec.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <unistd.h>

/* Read-only diagnostics do not claim addresses or transmit core announcements. */
int pc_cec_monitor(const char *path) {
    glob_t paths = {0};
    int found = -1;
    if (glob(path[0] ? path : "/dev/cec*", 0, NULL, &paths)) return -ENOENT;
    for (size_t i = 0; i < paths.gl_pathc; i++) {
        int fd = open(paths.gl_pathv[i], O_RDWR | O_NONBLOCK | O_CLOEXEC);
        uint16_t pa = CEC_PHYS_ADDR_INVALID;
        if (fd < 0) continue;
        if (ioctl(fd, CEC_ADAP_G_PHYS_ADDR, &pa) || pa == CEC_PHYS_ADDR_INVALID) { close(fd); continue; }
        if (found >= 0) { close(fd); close(found); globfree(&paths); return -EEXIST; }
        uint32_t mode = CEC_MODE_NO_INITIATOR | CEC_MODE_MONITOR_ALL;
        if (ioctl(fd, CEC_S_MODE, &mode)) {
            mode = CEC_MODE_NO_INITIATOR | CEC_MODE_MONITOR;
            if (ioctl(fd, CEC_S_MODE, &mode)) { close(fd); continue; }
        }
        fprintf(stderr, "CEC monitor=%s pa=%04x\n", paths.gl_pathv[i], pa);
        found = fd;
    }
    globfree(&paths);
    return found < 0 ? -ENODEV : found;
}

void pc_cec_monitor_close(int fd) { close(fd); }

/* Open only an unambiguous adapter with the expected (proxy) PA. The kernel
 * owns core query replies, using this same PA, version and OSD name. */
int pc_cec_open(const char *path, uint16_t expected, uint8_t *logical, int playback) {
    glob_t paths = {0};
    int found = -1;
    if (glob(path[0] ? path : "/dev/cec*", 0, NULL, &paths)) return -ENOENT;
    for (size_t i = 0; i < paths.gl_pathc; i++) {
        int fd = open(paths.gl_pathv[i], O_RDWR | O_NONBLOCK | O_CLOEXEC);
        struct cec_caps caps = {0};
        uint16_t pa = CEC_PHYS_ADDR_INVALID;
        if (fd < 0) continue;
        if (ioctl(fd, CEC_ADAP_G_CAPS, &caps) ||
            ioctl(fd, CEC_ADAP_G_PHYS_ADDR, &pa) || pa != expected ||
            (caps.capabilities & (CEC_CAP_TRANSMIT | CEC_CAP_LOG_ADDRS)) !=
                (CEC_CAP_TRANSMIT | CEC_CAP_LOG_ADDRS)) { close(fd); continue; }
        if (found >= 0) { close(found); close(fd); globfree(&paths); return -EEXIST; }
        found = fd;
        fprintf(stderr, "CEC adapter=%s driver=%s name=%s pa=%04x caps=%08x\n",
            paths.gl_pathv[i], caps.driver, caps.name, pa, caps.capabilities);
    }
    globfree(&paths);
    if (found < 0) return -ENODEV;
    uint32_t mode = CEC_MODE_EXCL_INITIATOR | CEC_MODE_EXCL_FOLLOWER;
    struct cec_log_addrs la = {0};
    if (ioctl(found, CEC_S_MODE, &mode)) goto fail;
    /* Refuse to steal another application's configured logical identities. */
    if (ioctl(found, CEC_ADAP_G_LOG_ADDRS, &la)) goto fail;
    if (la.num_log_addrs) {
        if (strncmp(la.osd_name, "Windows PC", sizeof(la.osd_name)) || la.vendor_id != CEC_VENDOR_ID_NONE ||
            la.num_log_addrs != 1) { errno = EBUSY; goto fail; }
        /* Our previous process may have died without releasing its identity. */
        struct cec_log_addrs empty = {0};
        if (ioctl(found, CEC_ADAP_S_LOG_ADDRS, &empty)) goto fail;
    }
    memset(&la, 0, sizeof(la));
    la.num_log_addrs = 1;
    la.cec_version = CEC_OP_CEC_VERSION_1_4;
    la.vendor_id = CEC_VENDOR_ID_NONE;
    /* Three existing consoles can occupy all playback slots. The default
     * proxy is a processor (specific-use LA); never evict a console. */
    la.primary_device_type[0] = playback ? CEC_OP_PRIM_DEVTYPE_PLAYBACK : CEC_OP_PRIM_DEVTYPE_PROCESSOR;
    la.log_addr_type[0] = playback ? CEC_LOG_ADDR_TYPE_PLAYBACK : CEC_LOG_ADDR_TYPE_SPECIFIC;
    memcpy(la.osd_name, "Windows PC", 11);
    /* Blocking claim is bounded by the framework's finite address polling. */
    int flags = fcntl(found, F_GETFL);
    if (flags < 0 || fcntl(found, F_SETFL, flags & ~O_NONBLOCK)) goto fail;
    if (ioctl(found, CEC_ADAP_S_LOG_ADDRS, &la)) goto fail;
    if (fcntl(found, F_SETFL, flags)) goto fail;
    if (!la.log_addr_mask || la.log_addr[0] >= 15) { errno = EADDRNOTAVAIL; goto fail; }
    /* Discard configuration events caused by our own initial claim. */
    struct cec_event event;
    while (!ioctl(found, CEC_DQEVENT, &event)) { }
    if (errno != EAGAIN) goto fail;
    uint16_t current_pa = CEC_PHYS_ADDR_INVALID;
    if (ioctl(found, CEC_ADAP_G_PHYS_ADDR, &current_pa)) goto fail;
    if (current_pa != expected) { errno = ENOLINK; goto fail; }
    *logical = la.log_addr[0];
    return found;
fail: {
    int err = errno;
    close(found);
    return -err;
}}

void pc_cec_close(int fd) {
    struct cec_log_addrs la = {0};
    (void)ioctl(fd, CEC_ADAP_S_LOG_ADDRS, &la);
    close(fd);
}

/* Nonblocking submission; actual completion arrives on the receive queue.
 * A successful ioctl only means queued, never successful AV routing. */
int pc_cec_tx(int fd, const uint8_t *data, uint32_t len, uint16_t expected) {
    if (len < 2 || len > CEC_MAX_MSG_SIZE) return -EINVAL;
    uint16_t pa = CEC_PHYS_ADDR_INVALID;
    if (ioctl(fd, CEC_ADAP_G_PHYS_ADDR, &pa)) return -errno;
    if (pa != expected) return -ENOLINK;
    struct cec_msg msg = {0};
    msg.len = len;
    memcpy(msg.msg, data, len);
    if (ioctl(fd, CEC_TRANSMIT, &msg)) return -errno;
    return (int)msg.sequence;
}

/* 1 RX, 2 TX completion, 3 adapter state/queue loss, 0 empty, negative errno. */
int pc_cec_next(int fd, uint8_t *data, uint32_t *len, uint32_t *status,
                uint32_t *seq, uint16_t expected, uint8_t logical) {
    struct cec_event event = {0};
    if (!ioctl(fd, CEC_DQEVENT, &event)) {
        if (event.event == CEC_EVENT_STATE_CHANGE) {
            uint16_t pa = CEC_PHYS_ADDR_INVALID;
            struct cec_log_addrs la = {0};
            /* Initial queued events predate address claiming: inspect now. */
            if (ioctl(fd, CEC_ADAP_G_PHYS_ADDR, &pa)) return -errno;
            if (logical < 15 && (pa != expected || ioctl(fd, CEC_ADAP_G_LOG_ADDRS, &la) ||
                !(la.log_addr_mask & (1U << logical)))) return -ENOLINK;
            if (pa == CEC_PHYS_ADDR_INVALID) return -ENOLINK;
            return (event.flags & CEC_EVENT_FL_INITIAL_STATE) ? 4 : 3;
        }
        if (event.event == CEC_EVENT_LOST_MSGS) return 3;
        return 0;
    }
    if (errno != EAGAIN) return -errno;
    struct cec_msg msg = {0};
    if (ioctl(fd, CEC_RECEIVE, &msg)) return errno == EAGAIN ? 0 : -errno;
    if (msg.len > CEC_MAX_MSG_SIZE) return -EIO;
    *len = msg.len;
    *seq = msg.sequence;
    *status = msg.tx_status ? msg.tx_status : msg.rx_status;
    memcpy(data, msg.msg, msg.len);
    return msg.tx_status ? 2 : 1;
}
