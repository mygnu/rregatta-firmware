/* Linker script for the STM32F103C6T6 */
/* Total memory 32K */
MEMORY
{
  FLASH : ORIGIN = 0x08000000, LENGTH = 32K
  RAM : ORIGIN = 0x20000000, LENGTH = 10K
}
