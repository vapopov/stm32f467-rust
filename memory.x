/* Specify the memory areas */
MEMORY
{
  FLASH   (rx)     : ORIGIN = 0x8000000,    LENGTH = 1024K
  RAM     (xrw)    : ORIGIN = 0x20010000,   LENGTH = 240K
  /* RAM : ORIGIN = 0x20000000, LENGTH = 64K + 240K + 16K */
  EXTRAM  (xrw)    : ORIGIN = 0xC0000000,   LENGTH = 16M

  Memory_B1(xrw)   : ORIGIN = 0x2004C000, LENGTH = 0xA0
  Memory_B2(xrw)   : ORIGIN = 0x2004C0A0, LENGTH = 0xA0
}

/* This is where the call stack will be allocated. */
/* The stack is of the full descending type. */
/* NOTE Do NOT modify `_stack_start` unless you know what you are doing */
_stack_start = ORIGIN(RAM) + LENGTH(RAM);

/* Define output sections */
SECTIONS
{
  .lwip (NOLOAD) :
  {
    . = ALIGN(1);
    __SDRAM_START__ = .;
    *(.lwip)
    *(.lwip*)
    . = ALIGN(1);
    __SDRAM_END__ = .;
  } >EXTRAM

  .RxDecripSection (NOLOAD) : { *(.RxDescripSection) } >Memory_B1
  .TxDescripSection (NOLOAD) : { *(.TxDescripSection) } >Memory_B2
}