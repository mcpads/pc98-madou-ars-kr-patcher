//! Static compositor audit for the Demo opening (`ARS_DEMO.OVL`).
//!
//! This module is intentionally game/version specific. It anchors the twenty
//! `OP*.CNS` filename loads, associates each opening phase with the buffers
//! populated by those loads, and recovers immediate source/destination geometry
//! at the proven graphics-copy routines. A recovered source rectangle proves
//! that the compositor reads those source pixels; it does not prove that the
//! pixels survive later masks or become visible in a naturally executed scene.

use anyhow::{Result, bail};
use v30::{
    CallTarget, EffectiveAddressBase, EffectiveAddressDisplacement, Instruction, Operand,
    Register16, SegmentRegister, decode_bytes,
};

pub const OVERLAY_LOAD_OFFSET: u16 = 0x0100;
pub const SCREEN_ROW_BYTES: usize = 80;
pub const OP_RESOURCES: [&str; 20] = [
    "OP1.CNS", "OP2.CNS", "OP3.CNS", "OP4.CNS", "OP5.CNS", "OP6.CNS", "OP7.CNS", "OP8.CNS",
    "OP9.CNS", "OP10.CNS", "OP11.CNS", "OP12.CNS", "OP13.CNS", "OP14.CNS", "OP15.CNS", "OP16.CNS",
    "OP17.CNS", "OP18.CNS", "OP19.CNS", "OP20.CNS",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteSpan {
    pub offset: usize,
    pub length: usize,
    pub row_bytes: usize,
    pub rows: usize,
    pub planes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceMode {
    /// Source rows live in a 640-pixel/80-byte screen sheet.
    ScreenStride80,
    /// Source rows are contiguous and their width is supplied by `DX`.
    LinearContiguous,
    /// Source rows use the stride written to the Demo variable at `CS:67D6`.
    ConfiguredStride,
}

impl SourceMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::ScreenStride80 => "screen_stride_80",
            Self::LinearContiguous => "linear_contiguous",
            Self::ConfiguredStride => "configured_stride",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveredArgs {
    pub si: Option<u16>,
    pub di: Option<u16>,
    pub dx: Option<u16>,
    pub cx: Option<u16>,
    /// `mov ds, word [cs:NNNN]`, used by the direct linear-copy routine.
    pub ds_segment_var: Option<u16>,
    /// Immediate value written to the custom source-row stride at `CS:67D6`.
    pub source_row_bytes: Option<u16>,
}

impl RecoveredArgs {
    fn score(self) -> (usize, usize) {
        (
            [self.si, self.di, self.dx, self.cx]
                .into_iter()
                .flatten()
                .count(),
            usize::from(self.ds_segment_var.is_some())
                + usize::from(self.source_row_bytes.is_some()),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoBlit {
    pub phase: u8,
    pub call_offset: u16,
    pub routine_offset: u16,
    pub routine: &'static str,
    pub resource: Option<&'static str>,
    pub source_mode: SourceMode,
    pub args: RecoveredArgs,
    pub source_rect: Option<Rect>,
    pub source_span: Option<ByteSpan>,
    /// Union of every source rectangle reached by one proven finite state
    /// transition. This is deliberately separate from an immediate rectangle.
    pub source_envelope: Option<Rect>,
    pub destination_rect: Option<Rect>,
}

impl DemoBlit {
    pub fn source_resolved(&self) -> bool {
        self.resource.is_some()
            && (self.source_rect.is_some()
                || self.source_span.is_some()
                || self.source_envelope.is_some())
    }

    pub fn source_state_bounded(&self) -> bool {
        self.source_envelope.is_some()
    }

    pub fn source_resolution_label(&self) -> &'static str {
        if self.source_rect.is_some() || self.source_span.is_some() {
            "static_immediates"
        } else if self.source_envelope.is_some() {
            "state_bounded_union"
        } else {
            "unresolved"
        }
    }

    pub fn destination_resolved(&self) -> bool {
        self.destination_rect.is_some()
    }

    pub fn fully_resolved(&self) -> bool {
        self.source_resolved() && self.destination_resolved()
    }

    pub fn source_unresolved_reason(&self) -> Option<&'static str> {
        if self.resource.is_none() {
            Some("direct source segment is not a mapped OP buffer")
        } else if self.source_rect.is_none()
            && self.source_span.is_none()
            && self.source_envelope.is_none()
        {
            Some("source geometry is not immediate")
        } else {
            None
        }
    }

    pub fn destination_unresolved_reason(&self) -> Option<&'static str> {
        self.destination_rect
            .is_none()
            .then_some("destination geometry is not immediate")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoCompositorAudit {
    pub blits: Vec<DemoBlit>,
}

impl DemoCompositorAudit {
    pub fn source_resolved_count(&self) -> usize {
        self.blits
            .iter()
            .filter(|blit| blit.source_resolved())
            .count()
    }

    pub fn source_unresolved_count(&self) -> usize {
        self.blits.len() - self.source_resolved_count()
    }

    pub fn fully_resolved_count(&self) -> usize {
        self.blits
            .iter()
            .filter(|blit| blit.fully_resolved())
            .count()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceSlot {
    Set1,
    Set2,
    Mask,
    Direct,
}

#[derive(Debug, Clone, Copy)]
struct Routine {
    offset: u16,
    name: &'static str,
    slot: SourceSlot,
    mode: SourceMode,
    pixels_per_dx: usize,
}

#[derive(Debug, Clone, Copy)]
struct StateBoundedSource {
    call_offset: u16,
    routine_offset: u16,
    resource: &'static str,
    envelope: Rect,
}

const STATE_BOUNDED_SOURCES: [StateBoundedSource; 13] = [
    StateBoundedSource {
        call_offset: 0x2530,
        routine_offset: 0x5EFD,
        resource: "OP3.CNS",
        envelope: Rect {
            x: 32,
            y: 264,
            width: 32,
            height: 22,
        },
    },
    StateBoundedSource {
        call_offset: 0x254A,
        routine_offset: 0x5F47,
        resource: "OP1.CNS",
        envelope: Rect {
            x: 32,
            y: 264,
            width: 32,
            height: 22,
        },
    },
    StateBoundedSource {
        call_offset: 0x3589,
        routine_offset: 0x5E84,
        resource: "OP7.CNS",
        envelope: Rect {
            x: 320,
            y: 200,
            width: 304,
            height: 200,
        },
    },
    StateBoundedSource {
        call_offset: 0x35BC,
        routine_offset: 0x5FE8,
        resource: "OP8.CNS",
        envelope: Rect {
            x: 0,
            y: 0,
            width: 320,
            height: 200,
        },
    },
    StateBoundedSource {
        call_offset: 0x3600,
        routine_offset: 0x5FE8,
        resource: "OP8.CNS",
        envelope: Rect {
            x: 0,
            y: 0,
            width: 312,
            height: 200,
        },
    },
    StateBoundedSource {
        call_offset: 0x372D,
        routine_offset: 0x5ED0,
        resource: "OP8.CNS",
        envelope: Rect {
            x: 320,
            y: 0,
            width: 320,
            height: 360,
        },
    },
    StateBoundedSource {
        call_offset: 0x3778,
        routine_offset: 0x5EFD,
        resource: "OP9.CNS",
        envelope: Rect {
            x: 320,
            y: 200,
            width: 320,
            height: 200,
        },
    },
    StateBoundedSource {
        call_offset: 0x37FD,
        routine_offset: 0x5ED0,
        resource: "OP11.CNS",
        envelope: Rect {
            x: 0,
            y: 0,
            width: 320,
            height: 336,
        },
    },
    StateBoundedSource {
        call_offset: 0x3EF9,
        routine_offset: 0x5ED0,
        resource: "OP13.CNS",
        envelope: Rect {
            x: 320,
            y: 0,
            width: 320,
            height: 192,
        },
    },
    StateBoundedSource {
        call_offset: 0x3F16,
        routine_offset: 0x5FA0,
        resource: "OP12.CNS",
        envelope: Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 112,
        },
    },
    StateBoundedSource {
        call_offset: 0x3F40,
        routine_offset: 0x5C15,
        resource: "OP14.CNS",
        envelope: Rect {
            x: 0,
            y: 96,
            width: 320,
            height: 104,
        },
    },
    StateBoundedSource {
        call_offset: 0x3F61,
        routine_offset: 0x5ED0,
        resource: "OP13.CNS",
        envelope: Rect {
            x: 0,
            y: 40,
            width: 64,
            height: 56,
        },
    },
    StateBoundedSource {
        call_offset: 0x48BE,
        routine_offset: 0x5ED0,
        resource: "OP13.CNS",
        envelope: Rect {
            x: 0,
            y: 200,
            width: 128,
            height: 200,
        },
    },
];

const ROUTINES: [Routine; 10] = [
    Routine {
        offset: 0x5E84,
        name: "set1_copy_words",
        slot: SourceSlot::Set1,
        mode: SourceMode::ScreenStride80,
        pixels_per_dx: 16,
    },
    Routine {
        offset: 0x5ED0,
        name: "set2_copy_words",
        slot: SourceSlot::Set2,
        mode: SourceMode::ScreenStride80,
        pixels_per_dx: 16,
    },
    Routine {
        offset: 0x5EFD,
        name: "mask_and_words",
        slot: SourceSlot::Mask,
        mode: SourceMode::ScreenStride80,
        pixels_per_dx: 16,
    },
    Routine {
        offset: 0x5F47,
        name: "set1_or_words",
        slot: SourceSlot::Set1,
        mode: SourceMode::ScreenStride80,
        pixels_per_dx: 16,
    },
    Routine {
        offset: 0x5FA0,
        name: "set1_copy_bytes",
        slot: SourceSlot::Set1,
        mode: SourceMode::ScreenStride80,
        pixels_per_dx: 8,
    },
    Routine {
        offset: 0x5FE8,
        name: "set2_copy_bytes",
        slot: SourceSlot::Set2,
        mode: SourceMode::ScreenStride80,
        pixels_per_dx: 8,
    },
    Routine {
        offset: 0x6015,
        name: "mask_and_bytes",
        slot: SourceSlot::Mask,
        mode: SourceMode::ScreenStride80,
        pixels_per_dx: 8,
    },
    Routine {
        offset: 0x6057,
        name: "set1_or_bytes",
        slot: SourceSlot::Set1,
        mode: SourceMode::ScreenStride80,
        pixels_per_dx: 8,
    },
    Routine {
        offset: 0x5BE5,
        name: "direct_linear_copy_words",
        slot: SourceSlot::Direct,
        mode: SourceMode::LinearContiguous,
        pixels_per_dx: 16,
    },
    Routine {
        offset: 0x5C15,
        name: "configured_stride_copy_words",
        slot: SourceSlot::Mask,
        mode: SourceMode::ConfiguredStride,
        pixels_per_dx: 16,
    },
];

#[derive(Debug, Clone, Copy)]
struct LogicalRange {
    start: u16,
    end: u16,
}

impl LogicalRange {
    fn contains(self, logical: u16) -> bool {
        (self.start..=self.end).contains(&logical)
    }
}

#[derive(Debug, Clone, Copy)]
struct PhaseProfile {
    number: u8,
    ranges: [LogicalRange; 2],
    set1: &'static str,
    set2: &'static str,
    mask: &'static str,
}

// The first range owns each phase's state/loader code and the second owns its
// compositor routines/tables. These boundaries are decoded-OVL logical offsets
// (the in-memory image starts at 0x0100).
const PHASES: [PhaseProfile; 7] = [
    PhaseProfile {
        number: 1,
        ranges: [
            LogicalRange {
                start: 0x0494,
                end: 0x0594,
            },
            LogicalRange {
                start: 0x229B,
                end: 0x27D1,
            },
        ],
        set1: "OP1.CNS",
        set2: "OP2.CNS",
        mask: "OP3.CNS",
    },
    PhaseProfile {
        number: 2,
        ranges: [
            LogicalRange {
                start: 0x0E6F,
                end: 0x116C,
            },
            LogicalRange {
                start: 0x27D2,
                end: 0x330B,
            },
        ],
        set1: "OP4.CNS",
        set2: "OP5.CNS",
        mask: "OP6.CNS",
    },
    PhaseProfile {
        number: 3,
        ranges: [
            LogicalRange {
                start: 0x12A4,
                end: 0x1487,
            },
            LogicalRange {
                start: 0x330C,
                end: 0x37D5,
            },
        ],
        set1: "OP7.CNS",
        set2: "OP8.CNS",
        mask: "OP9.CNS",
    },
    PhaseProfile {
        number: 4,
        ranges: [
            LogicalRange {
                start: 0x156D,
                end: 0x17BD,
            },
            LogicalRange {
                start: 0x37D6,
                end: 0x3C09,
            },
        ],
        set1: "OP10.CNS",
        set2: "OP11.CNS",
        mask: "OP9.CNS",
    },
    PhaseProfile {
        number: 5,
        ranges: [
            LogicalRange {
                start: 0x18A4,
                end: 0x1A06,
            },
            LogicalRange {
                start: 0x3C0A,
                end: 0x4982,
            },
        ],
        set1: "OP12.CNS",
        set2: "OP13.CNS",
        mask: "OP14.CNS",
    },
    PhaseProfile {
        number: 6,
        ranges: [
            LogicalRange {
                start: 0x1BA0,
                end: 0x1CB4,
            },
            LogicalRange {
                start: 0x4983,
                end: 0x5214,
            },
        ],
        set1: "OP16.CNS",
        set2: "OP15.CNS",
        mask: "OP19.CNS",
    },
    PhaseProfile {
        number: 7,
        ranges: [
            LogicalRange {
                start: 0x1CFC,
                end: 0x1FFF,
            },
            LogicalRange {
                start: 0x5215,
                end: 0x52FF,
            },
        ],
        // Phase 7 retains the phase-6 four-plane buffers and replaces 679F.
        set1: "OP16.CNS",
        set2: "OP15.CNS",
        mask: "OP20.CNS",
    },
];

const RESOURCE_FILENAMES: [(u16, &str); 20] = [
    (0x0210, "op1.cns\0"),
    (0x0218, "op2.cns\0"),
    (0x0220, "op3.cns\0"),
    (0x0228, "op4.cns\0"),
    (0x0230, "op5.cns\0"),
    (0x0238, "op6.cns\0"),
    (0x0240, "op7.cns\0"),
    (0x0248, "op8.cns\0"),
    (0x0250, "op9.cns\0"),
    (0x0258, "op10.cns\0"),
    (0x0261, "op11.cns\0"),
    (0x026A, "op12.cns\0"),
    (0x0273, "op13.cns\0"),
    (0x027C, "op14.cns\0"),
    (0x0285, "op15.cns\0"),
    (0x028E, "op16.cns\0"),
    (0x0297, "op17.cns\0"),
    (0x02A0, "op18.cns\0"),
    (0x02A9, "op19.cns\0"),
    (0x02B2, "op20.cns\0"),
];

const RESOURCE_LOADS: [(u16, u16); 20] = [
    (0x039C, 0x0210),
    (0x03EB, 0x0218),
    (0x043A, 0x0220),
    (0x0D43, 0x0228),
    (0x0D92, 0x0230),
    (0x0DE1, 0x0238),
    (0x1172, 0x0240),
    (0x11C1, 0x0248),
    (0x1210, 0x0250),
    (0x1250, 0x0258),
    (0x14B8, 0x0261),
    (0x17EE, 0x026A),
    (0x1507, 0x0273),
    (0x183D, 0x027C),
    (0x1A0C, 0x0285),
    (0x1A5B, 0x028E),
    (0x1AAA, 0x0297),
    (0x1AEA, 0x02A0),
    (0x1B2A, 0x02A9),
    (0x1CBA, 0x02B2),
];

#[derive(Debug, Clone, Copy)]
enum BufferAnchor {
    Set1,
    Set2,
    Single(u16),
}

const RESOURCE_BUFFER_ANCHORS: [(&str, u16, BufferAnchor); 20] = [
    ("OP1.CNS", 0x03C8, BufferAnchor::Set1),
    ("OP2.CNS", 0x0417, BufferAnchor::Set2),
    ("OP3.CNS", 0x0467, BufferAnchor::Single(0x679F)),
    ("OP4.CNS", 0x0D6F, BufferAnchor::Set1),
    ("OP5.CNS", 0x0DBE, BufferAnchor::Set2),
    ("OP6.CNS", 0x0E0E, BufferAnchor::Single(0x679F)),
    ("OP7.CNS", 0x119E, BufferAnchor::Set1),
    ("OP8.CNS", 0x11ED, BufferAnchor::Set2),
    ("OP9.CNS", 0x123D, BufferAnchor::Single(0x679F)),
    ("OP10.CNS", 0x1495, BufferAnchor::Set1),
    ("OP11.CNS", 0x14E4, BufferAnchor::Set2),
    ("OP12.CNS", 0x181A, BufferAnchor::Set1),
    ("OP13.CNS", 0x17CB, BufferAnchor::Set2),
    ("OP14.CNS", 0x186A, BufferAnchor::Single(0x679F)),
    ("OP15.CNS", 0x1A38, BufferAnchor::Set2),
    ("OP16.CNS", 0x1A87, BufferAnchor::Set1),
    ("OP17.CNS", 0x1AD7, BufferAnchor::Single(0x6789)),
    ("OP18.CNS", 0x1B17, BufferAnchor::Single(0x6787)),
    ("OP19.CNS", 0x1B57, BufferAnchor::Single(0x679F)),
    ("OP20.CNS", 0x1CE7, BufferAnchor::Single(0x679F)),
];

const SET1_BUFFER_SIGNATURE: [u8; 28] = [
    0x2E, 0xA1, 0x8F, 0x67, 0x8E, 0xC0, 0x2E, 0x8B, 0x1E, 0x91, 0x67, 0x2E, 0x8B, 0x0E, 0x93, 0x67,
    0x2E, 0x8B, 0x16, 0x95, 0x67, 0xBF, 0x00, 0x00, 0xB4, 0x04, 0xCD, 0x7C,
];

const SET2_BUFFER_SIGNATURE: [u8; 28] = [
    0x2E, 0xA1, 0x97, 0x67, 0x8E, 0xC0, 0x2E, 0x8B, 0x1E, 0x99, 0x67, 0x2E, 0x8B, 0x0E, 0x9B, 0x67,
    0x2E, 0x8B, 0x16, 0x9D, 0x67, 0xBF, 0x00, 0x00, 0xB4, 0x04, 0xCD, 0x7C,
];

/// Audit all direct calls to the statically mapped Demo graphics-copy routines.
/// The profile is rejected if the exact filename/load anchors are absent.
pub fn audit_demo_compositor(decoded: &[u8]) -> Result<DemoCompositorAudit> {
    verify_demo_profile(decoded)?;
    verify_state_contract_anchors(decoded)?;
    let mut blits = Vec::new();
    for phase in PHASES {
        for range in phase.ranges {
            scan_range(decoded, phase, range, &mut blits)?;
        }
    }
    blits.sort_by_key(|blit| blit.call_offset);
    verify_state_bounded_sources(&blits)?;
    verify_direct_call_coverage(decoded, &blits)?;
    verify_op10_has_no_direct_pixel_consumer(decoded, &blits)?;
    Ok(DemoCompositorAudit { blits })
}

fn verify_state_contract_anchors(decoded: &[u8]) -> Result<()> {
    for (logical, expected) in [
        (
            0x0AD1,
            &[
                0x2E, 0xC7, 0x46, 0x0B, 0x00, 0x00, 0x2E, 0xC7, 0x46, 0x0D, 0x16, 0x00, 0x2E, 0xC7,
                0x46, 0x0F, 0xA6, 0x2A,
            ][..],
        ),
        (
            0x12F7,
            &[
                0x2E, 0xC7, 0x46, 0x0B, 0x00, 0x00, 0x2E, 0xC7, 0x46, 0x0D, 0x00, 0x00, 0x2E, 0xC7,
                0x46, 0x0F, 0xBA, 0x3E, 0x2E, 0xC7, 0x46, 0x11, 0x0A, 0x00,
            ][..],
        ),
        (
            0x134A,
            &[
                0x2E, 0xC7, 0x46, 0x0B, 0x00, 0x00, 0x2E, 0xC7, 0x46, 0x0D, 0x14, 0x00, 0x2E, 0xC7,
                0x46, 0x0F, 0x15, 0x1E, 0x2E, 0xC7, 0x46, 0x11, 0x14, 0x00, 0x2E, 0xC7, 0x46, 0x13,
                0x00, 0x00,
            ][..],
        ),
        (
            0x13C5,
            &[
                0x2E, 0xC7, 0x46, 0x0B, 0x00, 0x00, 0x2E, 0xC7, 0x46, 0x0D, 0x28, 0x32, 0x2E, 0xC6,
                0x46, 0x15, 0x00,
            ][..],
        ),
        (
            0x1431,
            &[
                0x2E, 0xC7, 0x46, 0x0B, 0xA8, 0x3E, 0x2E, 0xC7, 0x46, 0x0D, 0x30, 0x1E, 0x2E, 0xC7,
                0x46, 0x0F, 0x06, 0x00, 0x2E, 0xC7, 0x46, 0x11, 0x20, 0x1E, 0x2E, 0xC7, 0x46, 0x13,
                0x0E, 0x00, 0x2E, 0xC6, 0x46, 0x15, 0x01, 0x2E, 0xC6, 0x46, 0x16, 0x00,
            ][..],
        ),
        (
            0x15CB,
            &[
                0x2E, 0xC7, 0x46, 0x0B, 0x80, 0x2A, 0x2E, 0xC7, 0x46, 0x0D, 0x27, 0x28, 0x2E, 0xC6,
                0x46, 0x15, 0x00, 0x2E, 0xC6, 0x46, 0x16, 0x00,
            ][..],
        ),
        (
            0x1915,
            &[
                0x2E, 0xC7, 0x46, 0x0B, 0x94, 0x20, 0x2E, 0xC7, 0x46, 0x0D, 0xC0, 0x00, 0x2E, 0xC7,
                0x46, 0x0F, 0x00, 0x1E, 0x2E, 0xC7, 0x46, 0x11, 0x08, 0x00, 0x2E, 0xC7, 0x46, 0x13,
                0x80, 0x1B, 0x2E, 0xC6, 0x46, 0x15, 0x00, 0x2E, 0xC6, 0x46, 0x16, 0x00,
            ][..],
        ),
        (
            0x19C0,
            &[
                0x2E, 0xC7, 0x46, 0x0B, 0xA5, 0x2F, 0x2E, 0xC7, 0x46, 0x0D, 0x01, 0x00, 0x2E, 0xC7,
                0x46, 0x0F, 0x8E, 0x3E, 0x2E, 0xC6, 0x46, 0x16, 0x00,
            ][..],
        ),
        (
            0x358D,
            &[0x2E, 0x83, 0x6E, 0x0F, 0x02, 0x2E, 0xFF, 0x46, 0x11][..],
        ),
        (
            0x3F72,
            &[
                0x2E, 0x81, 0x46, 0x0B, 0x00, 0x05, 0x2E, 0x83, 0x6E, 0x0D, 0x10, 0x2E, 0x81, 0x6E,
                0x0F, 0x80, 0x02, 0x2E, 0x83, 0x46, 0x11, 0x10,
            ][..],
        ),
    ] {
        expect_profile_bytes(decoded, logical, expected)?;
    }

    let repeated = [0x68, 0x35].repeat(10);
    expect_profile_bytes(decoded, 0x3411, &repeated)?;
    expect_profile_bytes(decoded, 0x3425, &[0x97, 0x35, 0x97, 0x35])?;
    Ok(())
}

fn expect_profile_bytes(decoded: &[u8], logical: u16, expected: &[u8]) -> Result<()> {
    let start = logical_to_index(logical)?;
    let end = start
        .checked_add(expected.len())
        .ok_or_else(|| anyhow::anyhow!("profile anchor range overflow"))?;
    if decoded.get(start..end) != Some(expected) {
        bail!("ARS_DEMO state profile mismatch at logical offset 0x{logical:04X}");
    }
    Ok(())
}

fn verify_state_bounded_sources(blits: &[DemoBlit]) -> Result<()> {
    for profile in STATE_BOUNDED_SOURCES {
        let matches = blits
            .iter()
            .filter(|blit| {
                blit.call_offset == profile.call_offset
                    && blit.routine_offset == profile.routine_offset
                    && blit.resource == Some(profile.resource)
                    && blit.source_envelope == Some(profile.envelope)
            })
            .count();
        if matches != 1 {
            bail!(
                "ARS_DEMO state-bounded source profile at 0x{:04X} matched {matches} call(s)",
                profile.call_offset
            );
        }
    }
    if blits.iter().any(|blit| !blit.source_resolved()) {
        bail!("ARS_DEMO compositor retains an unclassified source call");
    }
    Ok(())
}

fn verify_direct_call_coverage(decoded: &[u8], blits: &[DemoBlit]) -> Result<()> {
    let targets = ROUTINES.map(|routine| routine.offset);
    let observed = direct_call_sites(decoded, &targets)?;
    let expected = blits
        .iter()
        .map(|blit| (blit.call_offset, blit.routine_offset))
        .collect::<Vec<_>>();
    if observed != expected {
        bail!(
            "ARS_DEMO mapped direct-call coverage drifted: observed {}, profiled {}",
            observed.len(),
            expected.len()
        );
    }
    Ok(())
}

fn verify_op10_has_no_direct_pixel_consumer(decoded: &[u8], blits: &[DemoBlit]) -> Result<()> {
    if blits.iter().any(|blit| blit.resource == Some("OP10.CNS")) {
        bail!("ARS_DEMO OP10 unexpectedly reaches a mapped pixel consumer");
    }

    for (address, expected) in [
        (0x678Fu16, 12usize),
        (0x6791, 11),
        (0x6793, 12),
        (0x6795, 11),
    ] {
        let needle = address.to_le_bytes();
        let actual = decoded
            .windows(needle.len())
            .filter(|window| *window == needle)
            .count();
        if actual != expected {
            bail!(
                "ARS_DEMO set-1 segment variable 0x{address:04X} reference count drifted: {actual}, expected {expected}"
            );
        }
    }

    let special_calls = direct_call_sites(decoded, &[0x6341])?;
    let expected_special =
        [0x2D57u16, 0x2D68, 0x2D79, 0x2D8A, 0x2D9B, 0x2DAC, 0x2DBD].map(|site| (site, 0x6341));
    if special_calls != expected_special {
        bail!("ARS_DEMO set-1 half-plane consumer call sites drifted");
    }

    let set1_targets = [0x5E84, 0x5F47, 0x5FA0, 0x6057, 0x6341];
    let phase4 = PHASES
        .iter()
        .find(|phase| phase.number == 4)
        .expect("phase 4 profile");
    if direct_call_sites(decoded, &set1_targets)?
        .iter()
        .any(|(site, _)| phase4.ranges.iter().any(|range| range.contains(*site)))
    {
        bail!("ARS_DEMO phase 4 unexpectedly calls a set-1 pixel consumer");
    }
    Ok(())
}

fn direct_call_sites(decoded: &[u8], targets: &[u16]) -> Result<Vec<(u16, u16)>> {
    let mut calls = Vec::new();
    for offset in 0..decoded.len().saturating_sub(2) {
        if decoded[offset] != 0xE8 {
            continue;
        }
        let logical = index_to_logical(offset)?;
        let instruction = match decode_bytes(&decoded[offset..]) {
            Ok(instruction) => instruction,
            Err(_) => continue,
        };
        let Instruction::Call {
            target: CallTarget::Rel16(relative),
        } = instruction.instruction
        else {
            continue;
        };
        let target = logical.wrapping_add(3).wrapping_add_signed(relative);
        if targets.contains(&target) {
            calls.push((logical, target));
        }
    }
    calls.sort_unstable();
    Ok(calls)
}

fn verify_demo_profile(decoded: &[u8]) -> Result<()> {
    for (logical, name) in RESOURCE_FILENAMES {
        let offset = logical_to_index(logical)?;
        let end = offset
            .checked_add(name.len())
            .ok_or_else(|| anyhow::anyhow!("resource-name range overflow"))?;
        if decoded.get(offset..end) != Some(name.as_bytes()) {
            bail!("ARS_DEMO profile mismatch at filename logical offset 0x{logical:04X}");
        }
    }
    for (logical, filename_offset) in RESOURCE_LOADS {
        let offset = logical_to_index(logical)?;
        let [lo, hi] = filename_offset.to_le_bytes();
        let signature = [0xBA, lo, hi, 0xB4, 0x00, 0xCD, 0x7C];
        let end = offset
            .checked_add(signature.len())
            .ok_or_else(|| anyhow::anyhow!("resource-load range overflow"))?;
        if decoded.get(offset..end) != Some(signature.as_slice()) {
            bail!("ARS_DEMO profile mismatch at resource load logical offset 0x{logical:04X}");
        }
    }
    for (resource, logical, anchor) in RESOURCE_BUFFER_ANCHORS {
        let offset = logical_to_index(logical)?;
        let matched = match anchor {
            BufferAnchor::Set1 => {
                decoded.get(offset..offset + SET1_BUFFER_SIGNATURE.len())
                    == Some(SET1_BUFFER_SIGNATURE.as_slice())
            }
            BufferAnchor::Set2 => {
                decoded.get(offset..offset + SET2_BUFFER_SIGNATURE.len())
                    == Some(SET2_BUFFER_SIGNATURE.as_slice())
            }
            BufferAnchor::Single(segment_var) => {
                let [lo, hi] = segment_var.to_le_bytes();
                let signature = [
                    0x2E, 0x8E, 0x06, lo, hi, 0xBF, 0x00, 0x00, 0xB4, 0x03, 0xCD, 0x7C,
                ];
                decoded.get(offset..offset + signature.len()) == Some(signature.as_slice())
            }
        };
        if !matched {
            bail!(
                "ARS_DEMO profile mismatch at {resource} buffer anchor logical offset 0x{logical:04X}"
            );
        }
    }
    Ok(())
}

fn scan_range(
    decoded: &[u8],
    phase: PhaseProfile,
    range: LogicalRange,
    out: &mut Vec<DemoBlit>,
) -> Result<()> {
    let start = logical_to_index(range.start)?;
    let end = logical_to_index(range.end)?;
    if end >= decoded.len() {
        bail!(
            "phase {} range 0x{:04X}..0x{:04X} exceeds decoded ARS_DEMO size",
            phase.number,
            range.start,
            range.end
        );
    }
    for offset in start..=end.saturating_sub(2) {
        if decoded[offset] != 0xE8 {
            continue;
        }
        let logical = index_to_logical(offset)?;
        let instruction = decode_bytes(&decoded[offset..])
            .map_err(|error| anyhow::anyhow!("decode call at 0x{logical:04X}: {error}"))?;
        let Instruction::Call {
            target: CallTarget::Rel16(relative),
        } = instruction.instruction
        else {
            continue;
        };
        let target = logical.wrapping_add(3).wrapping_add_signed(relative);
        let Some(routine) = ROUTINES.iter().find(|routine| routine.offset == target) else {
            continue;
        };
        let args = recover_args(decoded, offset, target)?;
        out.push(build_blit(phase, logical, *routine, args));
    }
    Ok(())
}

fn build_blit(
    phase: PhaseProfile,
    call_offset: u16,
    routine: Routine,
    args: RecoveredArgs,
) -> DemoBlit {
    let resource = match routine.slot {
        SourceSlot::Set1 => Some(phase.set1),
        SourceSlot::Set2 => Some(phase.set2),
        SourceSlot::Mask => Some(phase.mask),
        SourceSlot::Direct => direct_resource(args.ds_segment_var),
    };
    let width = args
        .dx
        .map(usize::from)
        .and_then(|dx| dx.checked_mul(routine.pixels_per_dx));
    let height = args.cx.map(usize::from);
    let destination_rect = match (args.di, width, height) {
        (Some(di), Some(width), Some(height)) => Some(screen_rect(di, width, height)),
        _ => None,
    };
    let (source_rect, source_span) = match (routine.mode, args.si, width, height) {
        (SourceMode::ScreenStride80, Some(si), Some(width), Some(height)) => {
            (Some(screen_rect(si, width, height)), None)
        }
        (SourceMode::LinearContiguous, Some(si), Some(width), Some(height)) => {
            let row_bytes = width / 8;
            // 0x5BE5 invokes its inner copy once for each B/R/G/I VRAM
            // segment without restoring SI. A single outer call therefore
            // consumes four adjacent source planes, not just the first one.
            let length = row_bytes
                .checked_mul(height)
                .and_then(|plane_bytes| plane_bytes.checked_mul(4));
            (
                (si == 0).then_some(Rect {
                    x: 0,
                    y: 0,
                    width,
                    height,
                }),
                length.map(|length| ByteSpan {
                    offset: usize::from(si),
                    length,
                    row_bytes,
                    rows: height,
                    planes: 4,
                }),
            )
        }
        (SourceMode::ConfiguredStride, Some(si), Some(width), Some(height)) => {
            let source_rect = args.source_row_bytes.and_then(|row_bytes| {
                (row_bytes != 0).then(|| strided_rect(si, usize::from(row_bytes), width, height))
            });
            (source_rect, None)
        }
        _ => (None, None),
    };
    let source_envelope = if source_rect.is_none() && source_span.is_none() {
        resource.and_then(|resource| {
            state_bounded_source(call_offset, routine.offset, resource)
                .map(|profile| profile.envelope)
        })
    } else {
        None
    };
    DemoBlit {
        phase: phase.number,
        call_offset,
        routine_offset: routine.offset,
        routine: routine.name,
        resource,
        source_mode: routine.mode,
        args,
        source_rect,
        source_span,
        source_envelope,
        destination_rect,
    }
}

fn state_bounded_source(
    call_offset: u16,
    routine_offset: u16,
    resource: &str,
) -> Option<StateBoundedSource> {
    STATE_BOUNDED_SOURCES.iter().copied().find(|profile| {
        profile.call_offset == call_offset
            && profile.routine_offset == routine_offset
            && profile.resource == resource
    })
}

fn screen_rect(offset: u16, width: usize, height: usize) -> Rect {
    strided_rect(offset, SCREEN_ROW_BYTES, width, height)
}

fn strided_rect(offset: u16, row_bytes: usize, width: usize, height: usize) -> Rect {
    let offset = usize::from(offset);
    Rect {
        x: (offset % row_bytes) * 8,
        y: offset / row_bytes,
        width,
        height,
    }
}

fn direct_resource(segment_var: Option<u16>) -> Option<&'static str> {
    match segment_var {
        Some(0x6789) => Some("OP17.CNS"),
        Some(0x6787) => Some("OP18.CNS"),
        Some(0x679F) => Some("OP20.CNS"),
        _ => None,
    }
}

fn recover_args(decoded: &[u8], call_offset: usize, target: u16) -> Result<RecoveredArgs> {
    let start = call_offset.saturating_sub(48);
    let mut best = RecoveredArgs {
        si: None,
        di: None,
        dx: None,
        cx: None,
        ds_segment_var: None,
        source_row_bytes: None,
    };
    for candidate_start in start..call_offset {
        if !matches!(decoded[candidate_start], 0xB8..=0xBF) {
            continue;
        }
        let mut cursor = candidate_start;
        let mut state = RecoveredArgs {
            si: None,
            di: None,
            dx: None,
            cx: None,
            ds_segment_var: None,
            source_row_bytes: None,
        };
        let mut valid = true;
        while cursor <= call_offset {
            let logical = index_to_logical(cursor)?;
            let instruction = match decode_bytes(&decoded[cursor..]) {
                Ok(instruction) => instruction,
                Err(_) => {
                    valid = false;
                    break;
                }
            };
            let next = cursor.saturating_add(instruction.byte_len);
            if instruction.byte_len == 0 || next > call_offset.saturating_add(3) {
                valid = false;
                break;
            }
            if cursor == call_offset {
                valid = matches!(
                    instruction.instruction,
                    Instruction::Call {
                        target: CallTarget::Rel16(relative)
                    } if logical.wrapping_add(3).wrapping_add_signed(relative) == target
                );
                break;
            }
            if next > call_offset
                || !apply_preamble_instruction(&mut state, &instruction.instruction)
            {
                valid = false;
                break;
            }
            cursor = next;
        }
        if valid && state.score() > best.score() {
            best = state;
        }
    }
    Ok(best)
}

fn apply_preamble_instruction(state: &mut RecoveredArgs, kind: &Instruction) -> bool {
    match kind {
        Instruction::Mov { dest, src } => {
            match (dest, src) {
                (Operand::Reg16(reg), Operand::Imm16(value)) => set_reg(state, *reg, Some(*value)),
                (Operand::Reg16(reg), _) => set_reg(state, *reg, None),
                (Operand::Sreg(SegmentRegister::DS), Operand::Mem(mem))
                    if mem.segment() == Some(SegmentRegister::CS)
                        && mem.base() == EffectiveAddressBase::Direct =>
                {
                    state.ds_segment_var = direct_address(*mem);
                }
                (Operand::Sreg(SegmentRegister::DS), _) => state.ds_segment_var = None,
                (Operand::Mem(mem), Operand::Imm16(value))
                    if mem.segment() == Some(SegmentRegister::CS)
                        && mem.base() == EffectiveAddressBase::Direct
                        && direct_address(*mem) == Some(0x67D6) =>
                {
                    state.source_row_bytes = Some(*value);
                }
                _ => {}
            }
            true
        }
        Instruction::Push { .. } | Instruction::Nop => true,
        Instruction::Pop { dest } => {
            match dest {
                Operand::Reg16(reg) => set_reg(state, *reg, None),
                Operand::Sreg(SegmentRegister::DS) => state.ds_segment_var = None,
                _ => {}
            }
            true
        }
        _ => false,
    }
}

fn direct_address(memory: v30::EffectiveAddress) -> Option<u16> {
    match memory.displacement() {
        EffectiveAddressDisplacement::Absolute(address) => Some(address),
        EffectiveAddressDisplacement::Signed(_) => None,
    }
}

fn set_reg(state: &mut RecoveredArgs, reg: Register16, value: Option<u16>) {
    if reg == Register16::SI {
        state.si = value;
    } else if reg == Register16::DI {
        state.di = value;
    } else if reg == Register16::DX {
        state.dx = value;
    } else if reg == Register16::CX {
        state.cx = value;
    }
}

fn logical_to_index(logical: u16) -> Result<usize> {
    logical
        .checked_sub(OVERLAY_LOAD_OFFSET)
        .map(usize::from)
        .ok_or_else(|| anyhow::anyhow!("logical offset 0x{logical:04X} precedes overlay load"))
}

fn index_to_logical(index: usize) -> Result<u16> {
    let index = u16::try_from(index)?;
    OVERLAY_LOAD_OFFSET
        .checked_add(index)
        .ok_or_else(|| anyhow::anyhow!("decoded offset exceeds 16-bit overlay space"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_pc98_byte_offsets_to_pixel_rectangles() {
        assert_eq!(
            screen_rect(0x7080, 96, 40),
            Rect {
                x: 0,
                y: 360,
                width: 96,
                height: 40,
            }
        );
        assert_eq!(
            screen_rect(0x1E14, 320, 200),
            Rect {
                x: 160,
                y: 96,
                width: 320,
                height: 200,
            }
        );
    }

    #[test]
    fn recovers_immediate_arguments_and_direct_segment_variable() {
        let bytes = [
            0xBE, 0x00, 0x00, // mov si,0
            0x2E, 0x8E, 0x1E, 0x9F, 0x67, // mov ds,[cs:679f]
            0xBF, 0x14, 0x1E, // mov di,1e14
            0xBA, 0x14, 0x00, // mov dx,14
            0xB9, 0xC8, 0x00, // mov cx,c8
            0xE8, 0x00, 0x00, // call next
        ];
        let call = bytes.len() - 3;
        let call_logical = OVERLAY_LOAD_OFFSET + call as u16;
        let args = recover_args(&bytes, call, call_logical + 3).unwrap();
        assert_eq!(
            args,
            RecoveredArgs {
                si: Some(0),
                di: Some(0x1E14),
                dx: Some(0x14),
                cx: Some(0xC8),
                ds_segment_var: Some(0x679F),
                source_row_bytes: None,
            }
        );
    }

    #[test]
    fn direct_linear_copy_consumes_all_four_brgi_planes() {
        let phase = PHASES
            .iter()
            .copied()
            .find(|phase| phase.number == 7)
            .unwrap();
        let routine = ROUTINES
            .iter()
            .copied()
            .find(|routine| routine.offset == 0x5BE5)
            .unwrap();
        let blit = build_blit(
            phase,
            0x1D63,
            routine,
            RecoveredArgs {
                si: Some(0),
                di: Some(0x1E14),
                dx: Some(20),
                cx: Some(200),
                ds_segment_var: Some(0x679F),
                source_row_bytes: None,
            },
        );
        assert_eq!(blit.resource, Some("OP20.CNS"));
        assert_eq!(
            blit.source_span,
            Some(ByteSpan {
                offset: 0,
                length: 32_000,
                row_bytes: 40,
                rows: 200,
                planes: 4,
            })
        );
    }
}
