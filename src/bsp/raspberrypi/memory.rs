// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2018-2023 Andre Richter <andre.o.richter@gmail.com>

//! BSP Memory Management.

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

use core::{
    alloc::{GlobalAlloc, Layout},
    cell::UnsafeCell,
    mem, ptr,
};

use log::info;

/// The board's physical memory map.
#[rustfmt::skip]
pub(super) mod map {

    pub const GPIO_OFFSET:         usize = 0x0020_0000;
    pub const UART_OFFSET:         usize = 0x0020_1000;

    /// Physical devices.
    #[cfg(feature = "bsp_rpi3")]
    pub mod mmio {
        use super::*;

        pub const START:            usize =         0x3F00_0000;
        pub const GPIO_START:       usize = START + GPIO_OFFSET;
        pub const PL011_UART_START: usize = START + UART_OFFSET;
    }

    /// Physical devices.
    #[cfg(feature = "bsp_rpi4")]
    pub mod mmio {
        use super::*;

        pub const START:            usize =         0xFE00_0000;
        pub const GPIO_START:       usize = START + GPIO_OFFSET;
        pub const PL011_UART_START: usize = START + UART_OFFSET;
    }
}

extern "Rust" {
    static __heap_start: UnsafeCell<()>;
    static __heap_end_exclusive: UnsafeCell<()>;
}

struct Block {
    size: usize,
    next: *mut Block,
}

struct Heap {
    free_blocks: *mut Block,
}

#[global_allocator]
static mut MAIN_HEAP: Heap = Heap {
    free_blocks: ptr::null_mut(),
};

#[alloc_error_handler]
fn out_of_memory(layout: Layout) -> ! {
    panic!("kmalloc: out of memory while allocating {}", layout.size());
}

pub fn initialize_heap() {
    unsafe {
        MAIN_HEAP.init(
            __heap_start.get() as usize,
            __heap_end_exclusive.get() as usize,
        );
    }
}

pub unsafe fn kmalloc(size: usize) -> *mut u8 {
    MAIN_HEAP.malloc(size)
}

pub unsafe fn kmfree(ptr: *mut u8) {
    MAIN_HEAP.free(ptr);
}

unsafe impl GlobalAlloc for Heap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // TODO add mutex??
        kmalloc(layout.size())
        //self.malloc(layout.size())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        // TODO add mutex??
        kmfree(ptr);
        //self.free(ptr);
    }
}

impl Heap {
    pub unsafe fn init(&mut self, start: usize, end: usize) {
        let space = start as *mut Block;

        let size = end - start;
        info!(
            "kernel heap: using {:#x}, size {}MiB",
            start,
            size / 1024 / 1024
        );

        (*space).size = size;
        (*space).next = ptr::null_mut();

        self.free_blocks = space;
    }

    pub unsafe fn malloc(&mut self, mut size: usize) -> *mut u8 {
        let nextfree: *mut Block;
        let mut prev: *mut Block = ptr::null_mut();
        let mut cur: *mut Block = self.free_blocks;

        // Align the size to 8 bytes
        size += (8 - (size & 0x7)) & 0x7;
        let block_size = size + mem::size_of::<Block>();

        while !cur.is_null() {
            if (*cur).size >= block_size {
                // If the block can be split with enough room for another block struct and more than 8 bytes left over, then split it
                if (*cur).size >= block_size + mem::size_of::<Block>() + 8 {
                    nextfree = cur.cast::<u8>().add(block_size).cast();
                    (*nextfree).size = (*cur).size - block_size;
                    (*cur).size = block_size;

                    (*nextfree).next = (*cur).next;
                } else {
                    nextfree = (*cur).next;
                }
                (*cur).next = ptr::null_mut();

                if !prev.is_null() {
                    (*prev).next = nextfree;
                } else {
                    self.free_blocks = nextfree;
                }

                return cur.offset(1).cast();
            }

            prev = cur;
            cur = (*cur).next;
        }
        // Out Of Memory
        panic!("Kernel out of memory!  Halting...\n");
    }

    pub unsafe fn free(&mut self, ptr: *mut u8) {
        let mut prev: *mut Block = ptr::null_mut();
        let block: *mut Block = ptr.cast::<Block>().offset(-1);
        let mut cur: *mut Block = self.free_blocks;

        while !cur.is_null() {
            if (*cur).next == block {
                panic!("Double free detected at {:x}! Halting...\n", cur as usize);
            }

            if cur.cast::<u8>().add((*cur).size).cast() == block {
                // Merge the free'd block with the previous block
                (*cur).size += (*block).size;

                // If this block is adjacent to the next free block, then merge them
                if cur.cast::<u8>().add((*cur).size).cast() == (*cur).next {
                    (*cur).size += (*(*cur).next).size;
                    (*cur).next = (*(*cur).next).next;
                }
                return;
            }

            if cur >= block {
                // Insert the free'd block into the list
                if !prev.is_null() {
                    (*prev).next = block;
                } else {
                    self.free_blocks = block;
                }
                (*block).next = cur;

                // If this block is adjacent to the next free block, then merge them
                if block.cast::<u8>().add((*block).size).cast() == cur {
                    (*block).size += (*cur).size;
                    (*block).next = (*cur).next;
                }
                return;
            }

            prev = cur;
            cur = (*cur).next;
        }
    }
}
