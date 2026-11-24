# Game Console R36S. Hardkernel cfgload sources boot.scr on TF1.
setenv bootargs 'console=tty0 console=ttyS2,1500000n8 root=PARTUUID=36335030-02 rootfstype=squashfs rootwait ro'
# Without a serial cable, uboot.log on this partition shows the last stage reached.
setenv gizlet_log 0x08000000
setenv gizlet_stage script
env export -t ${gizlet_log} gizlet_stage gizlet_panel hwrev dtb_name bootargs
save mmc 1:1 ${gizlet_log} uboot.log ${filesize}
if fatload mmc 1:1 0x09000000 Image && fatload mmc 1:1 0x01f00000 r36s.dtb; then
    # A panel other than the original ST7703 is an overlay named mipi-panel.dtbo.
    setenv gizlet_panel none
    if fatload mmc 1:1 0x01e00000 mipi-panel.dtbo; then
        fdt addr 0x01f00000
        fdt resize 8192
        if fdt apply 0x01e00000; then setenv gizlet_panel mipi-panel.dtbo; else setenv gizlet_panel overlay-failed; fi
    fi
    setenv gizlet_stage booti
    env export -t ${gizlet_log} gizlet_stage gizlet_panel hwrev dtb_name bootargs
    save mmc 1:1 ${gizlet_log} uboot.log ${filesize}
    booti 0x09000000 - 0x01f00000
    setenv gizlet_stage booti-returned
else
    setenv gizlet_stage load-failed
fi
env export -t ${gizlet_log} gizlet_stage gizlet_panel hwrev dtb_name bootargs
save mmc 1:1 ${gizlet_log} uboot.log ${filesize}
