# Windows VM for the Lenovo Yoga C940-14IIL (`lenovo-yoga-c940-14iil-windows-vm`)

Runs the Windows 11 installation that is already on the laptop's NVMe drive (partition 3) in a KVM virtual machine,
shown in the GNOME desktop with virt-viewer / virt-manager (`qemu:///system`, domain `yoga-windows`), and usable
headless as a Windows build server (GitHub Actions runner, ssh on host port 2222).

## How it works

* **Virtual disk that looks like the original.** `yoga-windows-vm init` builds a small virtual GPT (head and tail image files
  in `/srv/yoga-windows-vm`) with the physical disk's GUID, the same ESP / MSR / Windows partition GUIDs and start sectors, and an ESP
  containing a *copy* of `\EFI\Microsoft` from the host ESP (read-only; the host ESP is not touched).  A device-mapper
  linear device `yoga-windows` concatenates head image, the real partition (found by PARTUUID, not by name) and tail image.
  The Debian partitions are never visible to the guest.  The BCD copy in the head image is patched
  (`yoga-windows-vm patch-bcd`): `hypervisorlaunchtype off` and `recoveryenabled off`.  Native Windows is not changed.
* **Overlay mode (default).** The dm device is mapped read-only and the guest writes to
  `/srv/yoga-windows-vm/overlay.qcow2`.  The real partition is never written.
* **Hook.** `/etc/libvirt/hooks/qemu.d/50-yoga-windows-vm` runs before QEMU starts: it unmounts `/windows` (refusing to start
  if it cannot) and builds the dm device; after QEMU stops it removes the dm device and loop devices and remounts `/windows`
  only if no QEMU for the domain is running and the mapping is gone.  While the VM runs the kernel also refuses to mount the
  partition (it is held by device-mapper), so the host cannot have it mounted read-write at the same time.
* **Guest hardware.** q35, OVMF (no Secure Boot, see below), swtpm TPM 2.0, 6 vCPUs host-passthrough (3 cores x 2 threads), 8 GiB,
  SATA disk, e1000e NIC through passt (host port 2222 -> guest 22), VGA display over SPICE (clipboard, file transfer),
  3 USB redirection channels, SPICE WebDAV channel, qemu-guest-agent channel, virtio-rng, balloon, a virtiofs share of
  `/srv/yoga-windows-vm/share`, host SMBIOS and the firmware's MSDM table (`-acpitable`, copied by `init` to
  `/srv/yoga-windows-vm/MSDM.aml`, readable only by root and libvirt-qemu) so the OEM key activates the VM.

## Set up

```
sudo apt install lenovo-yoga-c940-14iil-windows-vm
sudo adduser "$USER" libvirt            # log out and in; lets you use qemu:///system without sudo
# Windows must be fully shut down (not hibernated); Fast Startup and hibernation off (powercfg /h off).
sudo yoga-windows-vm init               # unmounts /windows, builds the images, defines the VM, remounts /windows
yoga-windows-vm launch                  # or the "Windows (VM)" launcher in GNOME
```

Commands: `yoga-windows-vm status | start [--viewer] | view | launch | stop | mode | autostart | remount | reset-overlay | patch-bcd`.
Configuration is `/etc/yoga-windows-vm/yoga-windows-vm.conf`.

## Windows build server

* Headless: `sudo yoga-windows-vm start` (no viewer needed).  ssh: `ssh -p 2222 ai@<host>` (the guest's existing sshd; the
  dev box alias `bruce-yoga-win`).  Add the guest key to the dev box's known_hosts for `[host]:2222` first.  Do not rely
  on the name `bruce-yoga.local`: the guest has the same hostname as the host.
* Sizing: 6 vCPU / 8 GiB leaves 2 threads and about 7 GiB for the Debian runner; change `<vcpu>`, `<memory>` and `<topology>` in
  `/usr/share/yoga-windows-vm/yoga-windows.xml.in` and run `sudo yoga-windows-vm mode overlay` (or `raw`) to redefine.
  Disk cache is `cache=none io=native` (no double caching, safe with the raw partition).
* **Autostart**: `sudo yoga-windows-vm autostart on` enables `yoga-windows-vm.service` (default OFF).  It starts after
  `local-fs.target` (so fstab has mounted `/windows`; the hook unmounts it) and is stopped *before* libvirtd and
  libvirt-guests at host shutdown: ACPI shutdown, wait `SHUTDOWN_WAIT` (180 s), destroy only as last resort.  A build server
  wants raw mode first; do not enable autostart before choosing a mode.
* **Windows power button.** Windows ignores the ACPI power button on a laptop (action Sleep).  For the graceful `stop` set
  once, inside Windows: `powercfg /setacvalueindex scheme_current sub_buttons pbuttonaction 3` and `powercfg /setactive scheme_current`
  (this changes native Windows too).  Alternatively `ssh -p 2222 ai@host shutdown /s /t 0` before `stop`.
* Do not start the runner loop or run the JIT minting script while testing the VM.

## Overlay or raw?  (Bruce decides; raw is NOT enabled)

| | overlay (default) | raw |
|---|---|---|
| real partition written | never | yes, in place |
| Windows state | separate from native boot; changes live in the qcow2 overlay (grows; `reset-overlay` discards it) | shared with native boot |
| after a Windows update / installs | lost on reset | kept |
| native <-> VM switching | no effect on native | drivers, NIC and display adapters change each time Windows sees different hardware; activation may re-evaluate |
| suitability as build server | poor (state lost) | good |

To switch (VM must be off): make a backup of partition 3 first (for example `sudo dd if=/dev/disk/by-partuuid/91727e4a-6ca4-4e78-a580-f823b82af2ec bs=4M status=progress | zstd > backup.zst`, with `/windows` unmounted),
then `sudo yoga-windows-vm mode raw --enable-raw-writes`.  This maps the dm device read-write and redefines the domain with the dm device as
the disk.  Back with `sudo yoga-windows-vm mode overlay`.  The ESP and BCD copy always stay in the head image (the host ESP is never written), so
native boot is unaffected by what the guest does to its BCD.  Never run the VM and a native boot of the same partition without a clean Windows shutdown in between.

## Sharing files while the VM runs

`/windows` is unavailable while the VM runs.  Options: ssh/scp to port 2222; the SPICE WebDAV channel (install the SPICE guest tools in Windows,
virt-viewer exposes a folder: File > Preferences > Share folder); or the virtiofs share of `/srv/yoga-windows-vm/share`
(needs WinFsp and the virtio-fs driver in Windows).  The virtio drivers staged in Windows' driver store (virtio-win 0.1.302) allow switching the
NIC to virtio, the disk to virtio-blk/scsi and the display to virtio-gpu by editing the XML template; not tried here.

## Verified (2026-10-31, bruce-yoga, overlay mode)

* init builds the composite disk; GPT of the dm device verified (`sgdisk -v`), disk and partition GUIDs equal the physical ones.
* The VM boots to the Windows 11 lock screen (screenshot); Windows reports Licensed (`slmgr /dli`, read only) with the MSDM table passed through.
* Hook: `/windows` unmounted before start, dm device and loop devices removed and `/windows` remounted after stop; start refused when
  `/windows` is busy (nothing left mapped); `remount` refused while the VM runs; `mount /windows` fails while it runs.
* Headless start; ssh key authentication on port 2222; ACPI shutdown (after the power-button setting above; 24 s);
  `autostart on/off` toggles the unit.
* Display is SPICE (no viewer was driven from a GNOME session in this test: the devices are present in the domain XML only).

## Not verified / known problems

* virt-viewer in a real GNOME session: clipboard, resize and USB redirection have not been exercised (resize needs the SPICE guest tools
  and the QXL driver; the VGA adapter used now has fixed resolutions).  QXL needs `qemu-system-modules-spice` (a dependency).
* Secure Boot: OVMF with the Microsoft keys refuses the host's `bootmgfw.efi` (Access Denied, probably signed by the 2023 CA), so Secure Boot is off in the VM.
* Hyper-V enlightenments: with them (and VBS running in the guest) the boot loader hangs; they are removed.  Windows VBS is disabled by `hypervisorlaunchtype off` in the BCD copy.
* Windows Hello PIN does not work in the VM (different TPM); use the account password.
* libvirt cannot define an emulated NVMe disk bus, so SATA is used.
* Raw mode and autostart at boot were not enabled or run.  virtio drivers were not tried.
* The package was not installed from a built .deb on the laptop (files were copied by hand during development).
* `lenovo-yoga-c940-14iil-support` is only Recommended: on this machine its dependency `grub-large-console` is not installable, which blocks apt.
