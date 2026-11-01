# shellcheck shell=bash
# Shared by the libvirt hook and the yoga-windows-vm helper.  Sourced.
CONF=${YWV_CONF:-/etc/yoga-windows-vm/yoga-windows-vm.conf}
DOMAIN=yoga-windows
DM_NAME=yoga-windows
DM_DEV=/dev/mapper/$DM_NAME
LOCK=/run/yoga-windows-vm.lock
RUNDIR=/run/yoga-windows-vm
# shellcheck disable=SC1090
. "$CONF"
WIN_DEV=/dev/disk/by-partuuid/$WIN_PARTUUID

log() { echo "yoga-windows-vm: $*" >&2; }

# True while a QEMU process for the domain exists (no libvirt call, so it is
# safe inside a libvirt hook).
vm_running() { pgrep -f "qemu.*guest=$DOMAIN," >/dev/null 2>&1; }

win_mounted() {
    local maj_min
    maj_min=$(lsblk -dno MAJ:MIN "$WIN_DEV" 2>/dev/null | tr -d ' ') || return 1
    awk -v mm="$maj_min" '$3 == mm {f=1} END {exit !f}' "${YWV_MOUNTINFO:-/proc/self/mountinfo}"
}

dm_up_p() { dmsetup info "$DM_NAME" >/dev/null 2>&1; }

# Build the composite disk: virtual GPT head (with ESP), the real Windows
# partition, virtual GPT tail.  Read-only unless MODE=raw.
disk_up() {
    local ro=--readonly lopt=-r head tail hs ps ts
    if [ "$MODE" = raw ]; then ro=; lopt=; fi
    [ -b "$WIN_DEV" ] || { log "Windows partition $WIN_DEV not found"; return 1; }
    if [ ! -f "$DATA_DIR/head.img" ] || [ ! -f "$DATA_DIR/tail.img" ]; then log "run: yoga-windows-vm init"; return 1; fi
    if dm_up_p; then log "$DM_NAME already mapped; refusing"; return 1; fi
    mkdir -p "$RUNDIR"
    head=$(losetup $lopt -f --show "$DATA_DIR/head.img") || return 1
    tail=$(losetup $lopt -f --show "$DATA_DIR/tail.img") || { losetup -d "$head"; return 1; }
    echo "$head" >"$RUNDIR/head"; echo "$tail" >"$RUNDIR/tail"
    hs=$(( $(stat -c %s "$DATA_DIR/head.img") / 512 ))
    ts=$(( $(stat -c %s "$DATA_DIR/tail.img") / 512 ))
    ps=$(blockdev --getsz "$WIN_DEV")
    # dm opens the partition exclusively: this fails if anything still has it.
    printf '0 %s linear %s 0\n%s %s linear %s 0\n%s %s linear %s 0\n' \
        "$hs" "$head" "$hs" "$ps" "$(readlink -f "$WIN_DEV")" "$((hs+ps))" "$ts" "$tail" |
        dmsetup create "$DM_NAME" $ro || { disk_down_loops; return 1; }
}

disk_down_loops() {
    local l
    for l in head tail; do
        if [ -f "${RUNDIR:?}/${l:?}" ]; then
            losetup -d "$(cat "${RUNDIR:?}/${l:?}")" 2>/dev/null
            rm -f "${RUNDIR:?}/${l:?}"
        fi
    done
    return 0
}

disk_down() {
    local i
    # QEMU, libvirt or udev may still hold the device for a moment after the
    # guest exits: retry for up to 30 s.
    for ((i=0; i<60; i++)); do
        dm_up_p || break
        dmsetup remove "$DM_NAME" 2>/dev/null && break
        sleep 0.5
    done
    if dm_up_p; then log "cannot remove $DM_NAME (busy)"; return 1; fi
    disk_down_loops
}

# Unmount the host copy; refuse (non-zero) if it cannot be unmounted.
host_unmount() {
    win_mounted || return 0
    umount "$WIN_MOUNT" 2>/dev/null || true
    if win_mounted; then log "cannot unmount $WIN_MOUNT (busy?); VM will not start"; return 1; fi
}

# Remount only when the VM is gone and the mapping is released.
host_remount() {
    if vm_running; then log "VM is running; NOT remounting $WIN_MOUNT"; return 1; fi
    if dm_up_p; then log "$DM_NAME still mapped; NOT remounting $WIN_MOUNT"; return 1; fi
    win_mounted && return 0
    mount "$WIN_MOUNT" || { log "mount $WIN_MOUNT failed (Windows not cleanly shut down?)"; return 1; }
}
