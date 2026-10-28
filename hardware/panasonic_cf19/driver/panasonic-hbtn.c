// SPDX-License-Identifier: GPL-2.0-only
/*
 * Panasonic Toughbook CF-18/19 tablet button driver
 *
 * Based on panasonic-hbtn (C) 2012 Heiher, itself derived from
 * panasonic-laptop.c (C) 2002-2004 John Belmonte.  Reworked for Linux 6.12:
 * devm resource handling, void ->remove, drains the firmware event queue.
 *
 * This program is free software; you can redistribute it and/or modify it
 * under the terms of the GNU General Public License version 2 as published
 * by the Free Software Foundation.
 *
 * The firmware's TBTN device (_HID MAT001F or MAT0020) queues one code per
 * button edge and signals Notify(0x80).  HINF returns the next queued code.
 * Codes are 0x3n: bit 0 clear = press, bit 0 set = release, bits 3:1 = button.
 * Codes 0x32/0x33 are not a button: the EC sends them when the swivel screen
 * changes position; SINF then returns the tablet-mode state (reported as
 * SW_TABLET_MODE).  The current state is also readable as a sysfs attribute
 * (tablet_mode) on the ACPI device for desktop components that cannot see the
 * input switch.
 */
#include <linux/module.h>
#include <linux/acpi.h>
#include <linux/input.h>
#include <linux/input/sparse-keymap.h>
#include <linux/sysfs.h>

#define PCC_HBTN_NAME		"Panasonic Tablet Button Support"
#define PCC_HBTN_CLASS		"pcc"
#define PCC_HBTN_QUERY		"HINF"
#define PCC_HBTN_NOTIFY		0x80
#define PCC_HBTN_STATE		"SINF"
#define PCC_HBTN_MAX_DRAIN	16
#define PCC_HBTN_TABLET_SCAN	0x2

static bool debug;
module_param(debug, bool, 0644);
MODULE_PARM_DESC(debug, "Log raw HINF codes (default: off)");

static const struct acpi_device_id pcc_hbtn_ids[] = {
	{ "MAT001F", 0 },
	{ "MAT0020", 0 },
	{ }
};
MODULE_DEVICE_TABLE(acpi, pcc_hbtn_ids);

/* scancode = (HINF code & 0xe) */
static const struct key_entry pcc_hbtn_keymap[] = {
	{ KE_KEY, 0x4, { KEY_PROG1 } },			/* "security" button, Ctrl-Alt-Del equivalent (XF86Launch1) */
	{ KE_KEY, 0x6, { KEY_PROG3 } },		/* screen rotate: handled by convertible-helper (XF86Launch3; not bound by GNOME) */
	{ KE_KEY, 0x8, { KEY_ENTER } },			/* enter */
	{ KE_KEY, 0xa, { KEY_PROG2 } },			/* on-screen keyboard toggle (XF86Launch2; keycodes > 255 have no X keysym) */
	{ KE_END, 0 }
};

/*
 * Action buttons are reported as a single tap on the press edge so holding the
 * button does not trigger keyboard auto-repeat and repeat the action.  Enter
 * and rotate keep real press/release timing.
 */
static bool pcc_hbtn_tap_only(unsigned int scancode)
{
	return scancode == 0x4 || scancode == 0xa;
}

struct pcc_hbtn {
	struct input_dev *input;
};


/* SINF returns Package{ Integer }: non-zero when the screen is in tablet position */
static int pcc_hbtn_tablet_state(struct acpi_device *device)
{
	struct acpi_buffer buf = { ACPI_ALLOCATE_BUFFER, NULL };
	union acpi_object *obj;
	acpi_status status;
	int ret = -EIO;

	status = acpi_evaluate_object(device->handle, PCC_HBTN_STATE, NULL, &buf);
	if (ACPI_FAILURE(status))
		return -EIO;

	obj = buf.pointer;
	if (obj && obj->type == ACPI_TYPE_PACKAGE && obj->package.count >= 1 &&
	    obj->package.elements[0].type == ACPI_TYPE_INTEGER)
		ret = obj->package.elements[0].integer.value ? 1 : 0;

	kfree(buf.pointer);
	return ret;
}

static void pcc_hbtn_report_tablet(struct acpi_device *device, struct pcc_hbtn *pcc)
{
	int state = pcc_hbtn_tablet_state(device);

	if (state < 0)
		return;
	if (debug)
		dev_info(&device->dev, "SINF tablet state=%d\n", state);
	input_report_switch(pcc->input, SW_TABLET_MODE, state);
	input_sync(pcc->input);
}

static ssize_t tablet_mode_show(struct device *dev,
				struct device_attribute *attr, char *buf)
{
	int state = pcc_hbtn_tablet_state(to_acpi_device(dev));

	if (state < 0)
		return state;
	return sysfs_emit(buf, "%d\n", state);
}
static DEVICE_ATTR_RO(tablet_mode);

static struct attribute *pcc_hbtn_attrs[] = {
	&dev_attr_tablet_mode.attr,
	NULL
};
ATTRIBUTE_GROUPS(pcc_hbtn);

static void pcc_hbtn_notify(struct acpi_device *device, u32 event)
{
	struct pcc_hbtn *pcc = acpi_driver_data(device);
	int i;

	if (debug)
		dev_info(&device->dev, "notify event=0x%x\n", event);
	if (event != PCC_HBTN_NOTIFY)
		return;

	for (i = 0; i < PCC_HBTN_MAX_DRAIN; i++) {
		unsigned long long result;
		const struct key_entry *ke;
		acpi_status status;

		status = acpi_evaluate_integer(device->handle, PCC_HBTN_QUERY,
					       NULL, &result);
		if (ACPI_FAILURE(status)) {
			dev_err(&device->dev, "%s failed: %s\n", PCC_HBTN_QUERY,
				acpi_format_exception(status));
			return;
		}
		if (!result) {		/* queue empty */
			if (debug)
				dev_info(&device->dev, "HINF=0 (queue empty), drained %d\n", i);
			return;
		}

		if (debug)
			dev_info(&device->dev, "HINF=0x%llx\n", result);

		acpi_bus_generate_netlink_event(acpi_device_class(device),
						dev_name(&device->dev),
						PCC_HBTN_NOTIFY, result);

		if ((result & 0xe) == PCC_HBTN_TABLET_SCAN) {
			pcc_hbtn_report_tablet(device, pcc);
			continue;
		}

		ke = sparse_keymap_entry_from_scancode(pcc->input, result & 0xe);
		if (!ke) {
			dev_dbg(&device->dev, "unknown tablet button code 0x%llx\n",
				result);
			continue;
		}
		if (pcc_hbtn_tap_only(ke->code)) {
			if (!(result & 0x1))		/* press edge only */
				sparse_keymap_report_entry(pcc->input, ke, 1, true);
			continue;
		}
		sparse_keymap_report_entry(pcc->input, ke,
					   (result & 0x1) ? 0 : 1, false);
	}
}

static int pcc_hbtn_add(struct acpi_device *device)
{
	struct pcc_hbtn *pcc;
	struct input_dev *input;
	int err;

	pcc = devm_kzalloc(&device->dev, sizeof(*pcc), GFP_KERNEL);
	if (!pcc)
		return -ENOMEM;

	input = devm_input_allocate_device(&device->dev);
	if (!input)
		return -ENOMEM;

	input->name = PCC_HBTN_NAME;
	input->phys = "panasonic/hbtn0";
	input->id.bustype = BUS_HOST;
	input->id.vendor = 0x0001;
	input->id.product = 0x0001;
	input->id.version = 0x0100;

	input_set_capability(input, EV_SW, SW_TABLET_MODE);

	err = sparse_keymap_setup(input, pcc_hbtn_keymap, NULL);
	if (err)
		return err;

	err = input_register_device(input);
	if (err)
		return err;

	err = devm_device_add_group(&device->dev, &pcc_hbtn_group);
	if (err)
		return err;

	pcc->input = input;
	device->driver_data = pcc;
	pcc_hbtn_report_tablet(device, pcc);	/* initial switch state */
	strscpy(acpi_device_name(device), "TabletButton", sizeof(acpi_device_name(device)));
	strscpy(acpi_device_class(device), PCC_HBTN_CLASS, sizeof(acpi_device_class(device)));
	return 0;
}

static struct acpi_driver pcc_hbtn_driver = {
	.name	= PCC_HBTN_NAME,
	.class	= PCC_HBTN_CLASS,
	.ids	= pcc_hbtn_ids,
	.ops	= {
		.add	= pcc_hbtn_add,
		.notify	= pcc_hbtn_notify,
	},
};
module_acpi_driver(pcc_hbtn_driver);

MODULE_AUTHOR("Heiher; reworked for Linux 6.12");
MODULE_DESCRIPTION("ACPI tablet button driver for Panasonic CF-18/19 Toughbooks");
MODULE_LICENSE("GPL");
