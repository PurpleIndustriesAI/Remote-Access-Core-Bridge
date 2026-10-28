# Devices and hardware

Device inventory is a separate permission for each paired peer. It defaults to false, including when older settings already allow clipboard and files. Only the computer that owns the hardware can grant it. A remote request queues the existing local approval prompt and does not grant reciprocal hardware access.

The Windows inventory returns at most 64 present USB entries: device class, vendor and product IDs, and status. It omits serial numbers, instance paths, personal device names, file contents and pairing material. One query can run at a time, lasts at most 12 seconds, and has a 30 second cooldown. The existing certificate pins, transport verification and local owner checks still apply. Permission is checked again after inspection so revocation blocks the response.

The RAC settings panel separates USB tether networking from USB device inventory. Older running bridges display an unavailable notice and cannot offer the new permission controls. Raw USB forwarding, peripheral commands and iPhone screen control are not implemented by inventory permission.

For an iPhone attached to a remote client, first verify the paired RAC connection, install the reviewed feature on both clients, and approve inventory on the machine with the phone. Confirm that its report contains an Apple USB entry. Apple trust, developer transport, signing where applicable, and a separately reviewed device operation adapter are still required for control. Do not expose an Apple service, WDA endpoint or unrestricted command runner to the network.

Acceptance requires denial before approval, no grant inherited from clipboard or files, revocation, metadata limits, supported client detection, and a real approved remote USB report. Source checks do not prove client installation or remote control.
