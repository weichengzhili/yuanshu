# yuanshu
yuanshu（元枢） os 操作系统



flowchart TB
START["kernel_main"] --> PCI["pci_scan()"]
PCI --> ENUM["遍历 bus/dev/func<br/>读 vendor_id/device_id"]
ENUM --> TABLE["PCI 设备表"]

    START --> REG["driver_register_builtin()"]
    REG --> DRV_TABLE["驱动注册表<br/>[{name, vendor, device, probe}]"]

    TABLE --> MATCH["match()"]
    DRV_TABLE --> MATCH
    MATCH -->|"vendor+device 匹配"| PROBE["driver->probe(pci_dev)"]
    PROBE --> INIT["driver->init(dev)"]
    INIT --> IRQ_REG["irq_register(irq, handler)"]
    IRQ_REG --> READY["设备就绪"]

    READY --> NETIF["netif_register()"]
    READY --> BLKDEV["blkdev_register()"]