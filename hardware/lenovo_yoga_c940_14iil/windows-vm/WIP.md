# Windows VM: work in progress (stopped 2026-10-31, host taken down by Bruce)

Prototype files only; NOT packaged, no debian/ entries, no README, no tests yet.

Verified on bruce-yoga (overlay mode, p3 never written):
- `yoga-windows-vm init` builds head.img/tail.img (GPT with the real disk and partition GUIDs, ESP copy), dm-linear composite, qcow2 overlay; domain defines.
- libvirt hook (qemu.d) unmounts /windows before start, remounts after stop; dm removal needs a retry loop (added).
- AppArmor needs /etc/apparmor.d/local/abstractions/libvirt-qemu rule for MSDM.aml (conf/apparmor-libvirt-qemu).
- Debian needs qemu-system-modules-spice (+opengl) for QXL/SPICE.
- OVMF with Secure Boot (MS keys): bootmgfw gets Access Denied (likely 2023 CA); Secure Boot is therefore off in the XML.
- libvirt has no emulated NVMe disk bus: SATA is used.
- Windows virtio NIC has no driver: e1000e used; passt forwards host 2222 -> 22.

Open problem: boot reaches bootmgr/winload, first run gives 0xc0000225 recovery screen; with recoveryenabled=0 in the VM's BCD copy it hangs with CPU0 at a constant RIP in firmware-range memory, reading >120 GB from the disk, no Windows logo.
Next experiments: no hyperv enlightenments / no vTPM (was being tested), NVMe via qemu:commandline, plain CPU model, bootmgr debug.

Remaining: packaging (debian/, Depends on lenovo-yoga-c940-14iil-support), README (verified vs not, raw switch, file sharing, Windows build server section), tests, linters, PR.
Note: apt on bruce-yoga was blocked because lenovo-yoga-c940-14iil-support depends on grub-large-console not installable there.
