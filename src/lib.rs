mod range_inclusive;
pub use range_inclusive::RangeInclusive;
mod scrollbar;
mod y_pos;

use core::{fmt::Write, iter::once, mem::size_of, num::NonZeroU8};
use imgui::{Drag, Key, MouseButton, StyleColor, StyleVar, Ui, WindowHoveredFlags};
use scrollbar::Scrollbar;
use y_pos::{SignedYPos, YPos, YPosRaw};

// TODO:
// - Add an `access_rights` callback that returns whether a given address's access rights are
//  `None`, `ReadOnly` or `ReadWrite`.
// - Add editing... somehow

macro_rules! str_buf {
    ($buf: expr, $($args: tt)*) => {{
        use core::fmt::Write;
        $buf.clear();
        write!($buf, $($args)*).unwrap();
        &$buf
    }};
}

bitflags::bitflags! {
    pub struct Flags: u16 {
        // Creation flags
        const READ_ONLY = 1 << 0;
        const SHOW_VIEW_OPTIONS = 1 << 1;
        const SHOW_RANGE = 1 << 2;

        // Options
        const GRAY_OUT_ZEROS = 1 << 8;
        const UPPERCASE_HEX = 1 << 9;
        const LITTLE_ENDIAN_COLS = 1 << 10;
        const SHOW_HEXII = 1 << 11;
        const SHOW_ASCII = 1 << 12;
        const SHOW_DATA_PREVIEW = 1 << 13;
    }
}

impl Flags {
    const LAYOUT_MASK: Self = Self::from_bits_truncate(
        Flags::SHOW_VIEW_OPTIONS.bits()
            | Flags::SHOW_RANGE.bits()
            | Flags::SHOW_ASCII.bits()
            | Flags::SHOW_DATA_PREVIEW.bits(),
    );
}

pub type Addr = u64;
pub type WAddr = u128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DataPreviewBaseTy {
    I8,
    I16,
    I24,
    I32,
    I64,
    I128,
    F32,
    F64,
    C8,
    C16,
    C32,
}

impl DataPreviewBaseTy {
    fn allows_signed(self) -> bool {
        matches!(
            self,
            DataPreviewBaseTy::I8
                | DataPreviewBaseTy::I16
                | DataPreviewBaseTy::I24
                | DataPreviewBaseTy::I32
                | DataPreviewBaseTy::I64
                | DataPreviewBaseTy::I128
        )
    }

    fn allows_endianness(self) -> bool {
        !matches!(self, DataPreviewBaseTy::I8 | DataPreviewBaseTy::C8)
    }
}

impl AsRef<str> for DataPreviewBaseTy {
    fn as_ref(&self) -> &str {
        match self {
            DataPreviewBaseTy::I8 => "Int8",
            DataPreviewBaseTy::I16 => "Int16",
            DataPreviewBaseTy::I24 => "Int24",
            DataPreviewBaseTy::I32 => "Int32",
            DataPreviewBaseTy::I64 => "Int64",
            DataPreviewBaseTy::I128 => "Int128",
            DataPreviewBaseTy::F32 => "Float32",
            DataPreviewBaseTy::F64 => "Float64",
            DataPreviewBaseTy::C8 => "UTF-8",
            DataPreviewBaseTy::C16 => "UTF-16",
            DataPreviewBaseTy::C32 => "UTF-32",
        }
    }
}

struct DataPreviewTy {
    base: DataPreviewBaseTy,
    signed: bool,
    little_endian: bool,
}

pub struct MemoryEditor {
    cols: NonZeroU8,
    col_size: NonZeroU8,
    bytes_per_row: Addr,
    addr_digits: Option<NonZeroU8>,
    addr_range: RangeInclusive<Addr>,
    flags: Flags,

    scrollbar: Scrollbar,
    visible_data_rows: RangeInclusive<Addr>,

    selected_addr: Addr,
    addr_input_buffer: String,
    selected_addr_changed: bool,

    data_preview_ty: DataPreviewTy,

    str_buffer: String,
    layout: Option<Layout>,

    pub highlights: Vec<(RangeInclusive<Addr>, [f32; 4])>,
}

impl MemoryEditor {
    #[inline]
    pub fn new() -> Self {
        MemoryEditor {
            cols: NonZeroU8::new(16).unwrap(),
            col_size: NonZeroU8::new(1).unwrap(),
            bytes_per_row: 16,
            addr_digits: None,
            addr_range: (0, 0).into(),
            flags: Flags::SHOW_VIEW_OPTIONS
                | Flags::SHOW_RANGE
                | Flags::GRAY_OUT_ZEROS
                | Flags::UPPERCASE_HEX
                | Flags::SHOW_ASCII,

            scrollbar: Scrollbar::new(),
            visible_data_rows: (0, 0).into(),

            selected_addr: 0,
            addr_input_buffer: String::new(),
            selected_addr_changed: true,

            data_preview_ty: DataPreviewTy {
                base: DataPreviewBaseTy::I8,
                signed: false,
                little_endian: true,
            },

            str_buffer: String::new(),
            layout: None,

            highlights: Vec::new(),
        }
    }

    #[inline]
    pub fn calc_bytes_per_row(&self) -> Addr {
        self.cols.get() as Addr * self.col_size.get() as Addr
    }

    #[inline]
    pub fn set_cols(&mut self, value: NonZeroU8) {
        self.cols = value;
        self.bytes_per_row = self.calc_bytes_per_row();
        self.layout = None;
    }

    #[inline]
    pub fn set_col_size(&mut self, value: NonZeroU8) {
        self.col_size = value;
        self.bytes_per_row = self.calc_bytes_per_row();
        self.layout = None;
    }

    #[inline]
    pub fn set_addr_digits(&mut self, value: Option<NonZeroU8>) {
        self.addr_digits = value;
        self.layout = None;
    }

    #[inline]
    pub fn set_addr_range(&mut self, value: RangeInclusive<Addr>) {
        self.addr_range = value;
        self.selected_addr = self.selected_addr.clamp(value.start, value.end);
        self.layout = None;
    }

    #[inline]
    pub fn set_flags(&mut self, flags: Flags) {
        if flags
            .symmetric_difference(self.flags)
            .intersects(Flags::LAYOUT_MASK)
        {
            self.layout = None;
        }
        self.flags = flags;
    }

    #[inline]
    pub fn set_read_only(&mut self, value: bool) {
        self.flags.set(Flags::READ_ONLY, value);
    }

    #[inline]
    pub fn set_show_view_options(&mut self, value: bool) {
        self.flags.set(Flags::SHOW_VIEW_OPTIONS, value);
        self.layout = None;
    }

    #[inline]
    pub fn set_show_range(&mut self, value: bool) {
        self.flags.set(Flags::SHOW_RANGE, value);
        self.layout = None;
    }

    #[inline]
    pub fn set_show_data_preview(&mut self, value: bool) {
        self.flags.set(Flags::SHOW_DATA_PREVIEW, value);
        self.layout = None;
    }

    #[inline]
    pub fn set_gray_out_zeros(&mut self, value: bool) {
        self.flags.set(Flags::GRAY_OUT_ZEROS, value);
    }

    #[inline]
    pub fn set_uppercase_hex(&mut self, value: bool) {
        self.flags.set(Flags::UPPERCASE_HEX, value);
    }

    #[inline]
    pub fn set_little_endian_cols(&mut self, value: bool) {
        self.flags.set(Flags::LITTLE_ENDIAN_COLS, value);
    }

    #[inline]
    pub fn set_show_hexii(&mut self, value: bool) {
        self.flags.set(Flags::SHOW_HEXII, value);
    }

    #[inline]
    pub fn set_show_ascii(&mut self, value: bool) {
        self.flags.set(Flags::SHOW_ASCII, value);
        self.layout = None;
    }
}

impl Default for MemoryEditor {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
struct Layout {
    addr_digits: NonZeroU8,
    total_rows: WAddr,

    data_row_height: f32,
    data_row_spacing: f32,
    data_row_height_with_spacing_int: YPos,
    data_row_height_with_spacing: f32,
    data_height_int: YPos,

    glyph_width: f32,
    hex_byte_width: f32,

    hex_col_width_with_spacing: f32,
    ascii_col_width_with_spacing: f32,

    hex_start_win_x: f32,
    hex_end_win_x: f32,
    ascii_sep_win_x: f32,
    ascii_start_win_x: f32,
    ascii_end_scrollbar_start_win_x: f32,

    item_spacing_x: f32,
    range_width: f32,
    addr_input_width: f32,
    scrollbar_size: f32,

    win_width: f32,

    footer_height: f32,
    data_preview_and_footer_height: f32,
}

impl MemoryEditor {
    #[inline]
    pub fn visible_addrs(&self, context_rows: Addr) -> RangeInclusive<Addr> {
        (
            self.addr_range.start
                + self.visible_data_rows.start.saturating_sub(context_rows) * self.bytes_per_row,
            self.addr_range
                .start
                .saturating_add(
                    self.visible_data_rows
                        .end
                        .saturating_add(context_rows)
                        .saturating_mul(self.bytes_per_row)
                        .saturating_add(self.bytes_per_row - 1),
                )
                .min(self.addr_range.end),
        )
            .into()
    }

    fn compute_layout(&mut self, ui: &Ui) {
        if self.layout.is_some() {
            return;
        }

        let addr_digits = self.addr_digits.unwrap_or_else(|| {
            NonZeroU8::new(
                ((Addr::BITS + 3 - self.addr_range.end.leading_zeros()) >> 2).max(1) as u8,
            )
            .unwrap()
        });

        let total_rows = ((self.addr_range.end - self.addr_range.start) as WAddr + 1)
            / self.bytes_per_row as WAddr;

        let style = unsafe { ui.style() };

        let data_row_height = ui.text_line_height();
        let data_row_spacing = style.item_spacing[1];
        let data_row_height_with_spacing_int = YPos::from(data_row_height + data_row_spacing);
        let data_row_height_with_spacing = data_row_height_with_spacing_int.into();

        let data_height_int = data_row_height_with_spacing_int * total_rows as YPosRaw;

        let glyph_width = ui.calc_text_size("0")[0];
        let hex_byte_width = ui.calc_text_size("00")[0];

        let data_col_spacing = style.item_spacing[0];
        let hex_col_width = hex_byte_width * self.col_size.get() as f32;
        let ascii_col_width = glyph_width * self.col_size.get() as f32;
        let hex_col_width_with_spacing = hex_col_width + data_col_spacing;
        let ascii_col_width_with_spacing = ascii_col_width + data_col_spacing;

        let data_addr_width = ui.calc_text_size(str_buf!(
            self.str_buffer,
            "{:0addr_digits$X}:",
            0,
            addr_digits = addr_digits.get() as usize
        ))[0];

        let v_spacer_width = style.item_spacing[0].max(glyph_width);

        let hex_start_win_x = data_addr_width + v_spacer_width;
        let (hex_end_win_x, ascii_sep_win_x, ascii_start_win_x, ascii_end_scrollbar_start_win_x) =
            if self.flags.contains(Flags::SHOW_ASCII) {
                let hex_end_win_x = hex_start_win_x
                    + hex_col_width_with_spacing * (self.cols.get() - 1) as f32
                    + hex_col_width;
                let ascii_start_win_x = hex_end_win_x + v_spacer_width;
                (
                    hex_end_win_x,
                    hex_end_win_x + v_spacer_width * 0.5 - 0.5,
                    ascii_start_win_x,
                    ascii_start_win_x + ascii_col_width_with_spacing * self.cols.get() as f32,
                )
            } else {
                let hex_end_win_x =
                    hex_start_win_x + hex_col_width_with_spacing * self.cols.get() as f32;
                (hex_end_win_x, hex_end_win_x, hex_end_win_x, hex_end_win_x)
            };
        let win_width = ascii_end_scrollbar_start_win_x + style.scrollbar_size;

        let mut data_preview_height = 0.0;

        if self.flags.contains(Flags::SHOW_DATA_PREVIEW) {
            data_preview_height += style.item_spacing[1] * 4.0 + ui.frame_height();
            match self.data_preview_ty.base {
                DataPreviewBaseTy::I8
                | DataPreviewBaseTy::I16
                | DataPreviewBaseTy::I24
                | DataPreviewBaseTy::I32
                | DataPreviewBaseTy::I64
                | DataPreviewBaseTy::I128 => {
                    data_preview_height += ui.text_line_height() * 3.0;
                }
                DataPreviewBaseTy::F32
                | DataPreviewBaseTy::F64
                | DataPreviewBaseTy::C8
                | DataPreviewBaseTy::C16
                | DataPreviewBaseTy::C32 => {
                    data_preview_height += ui.text_line_height();
                }
            }
        }

        let mut footer_x_remaining = win_width;
        let mut footer_line_height = 0.0_f32;
        let mut footer_height = style.item_spacing[1] * 2.0;

        macro_rules! add_footer_widget {
            ($width: expr, $height: expr$(, wrap $footer_x_remaining: ident)?) => {
                $(
                    if $footer_x_remaining < $width {
                        $footer_x_remaining = win_width;
                        footer_height += footer_line_height + style.item_spacing[1];
                        footer_line_height = 0.0;
                    }
                )*
                footer_line_height = footer_line_height.max($height);
                #[allow(unused_assignments)]
                {
                    footer_x_remaining -= $width + style.item_spacing[0];
                }
            };
        }

        if self.flags.contains(Flags::SHOW_VIEW_OPTIONS) {
            let options_size = {
                let size = ui.calc_text_size("Options...");
                [
                    size[0] + style.frame_padding[0] * 2.0,
                    size[1] + style.frame_padding[1] * 2.0,
                ]
            };
            add_footer_widget!(options_size[0], options_size[1]);
        }

        let range_width = if self.flags.contains(Flags::SHOW_RANGE) {
            let range_size = ui.calc_text_size(str_buf!(
                self.str_buffer,
                "Range {:0addr_digits$X}..{:0addr_digits$X}",
                self.addr_range.start,
                self.addr_range.end,
                addr_digits = addr_digits.get() as usize
            ));
            add_footer_widget!(range_size[0], range_size[1], wrap footer_x_remaining);
            range_size[0]
        } else {
            0.0
        };

        let addr_input_width = {
            let addr_text_size = ui.calc_text_size(str_buf!(
                self.str_buffer,
                "{:0addr_digits$X}",
                0,
                addr_digits = addr_digits.get() as usize
            ));
            let addr_size = [
                addr_text_size[0] + style.frame_padding[0] * 2.0,
                addr_text_size[1] + style.frame_padding[1] * 2.0,
            ];
            add_footer_widget!(addr_size[0], addr_size[1], wrap footer_x_remaining);
            addr_size[0]
        };

        footer_height += footer_line_height;

        self.layout = Some(Layout {
            addr_digits,
            total_rows,

            data_row_height,
            data_row_spacing,
            data_row_height_with_spacing_int,
            data_row_height_with_spacing,
            data_height_int,

            glyph_width,
            hex_byte_width,

            hex_col_width_with_spacing,
            ascii_col_width_with_spacing,

            hex_start_win_x,
            hex_end_win_x,
            ascii_sep_win_x,
            ascii_start_win_x,
            ascii_end_scrollbar_start_win_x,

            item_spacing_x: style.item_spacing[0],
            range_width,
            addr_input_width,
            scrollbar_size: style.scrollbar_size,

            win_width,

            footer_height,
            data_preview_and_footer_height: footer_height + data_preview_height,
        });
    }

    #[inline]
    pub fn window_width(&mut self, ui: &Ui) -> f32 {
        self.compute_layout(ui);
        let layout = self.layout.as_ref().unwrap();
        layout.ascii_end_scrollbar_start_win_x
            + layout.scrollbar_size
            + unsafe { ui.style().window_padding[0] } * 2.0
    }

    fn focus_on_selected_addr(&mut self, ui: &Ui) {
        let layout = self.layout.as_ref().unwrap();
        let content_height = ui.window_size()[1];

        let selected_row = self.selected_addr / self.bytes_per_row;
        let selection_start_scroll =
            layout.data_row_height_with_spacing_int * selected_row as YPosRaw;

        if self.scrollbar.scroll >= selection_start_scroll {
            self.scrollbar.scroll = selection_start_scroll;
        } else {
            let selection_end_scroll_minus_content_height = (selection_start_scroll
                + layout.data_row_height_with_spacing_int)
                .saturating_sub(content_height.into());
            if self.scrollbar.scroll <= selection_end_scroll_minus_content_height {
                self.scrollbar.scroll = selection_end_scroll_minus_content_height;
            }
        }
    }

    pub fn set_selected_addr(&mut self, addr: Addr) {
        self.selected_addr = addr.clamp(self.addr_range.start, self.addr_range.end);
        self.selected_addr_changed = true;
    }

    #[inline]
    pub fn handle_options_right_click(&mut self, ui: &Ui) {
        if self.flags.contains(Flags::SHOW_VIEW_OPTIONS)
            && ui.is_window_hovered_with_flags(WindowHoveredFlags::ROOT_AND_CHILD_WINDOWS)
            && ui.is_mouse_clicked(MouseButton::Right)
        {
            ui.open_popup("options");
        }
    }

    #[inline]
    pub fn draw_buffer_read_only(&mut self, ui: &Ui, window_title: Option<&str>, buffer: &[u8]) {
        assert!(
            buffer.len() as WAddr == (self.addr_range.end - self.addr_range.start) as WAddr + 1
        );
        let was_read_only = self.flags.contains(Flags::READ_ONLY);
        self.set_read_only(true);
        let base_addr = self.addr_range.start;
        self.draw_callbacks(
            ui,
            window_title,
            &mut &*buffer,
            move |buffer, addr| Some(buffer[(addr - base_addr) as usize]),
            move |_, _, _| {},
        );
        self.set_read_only(was_read_only);
    }

    #[inline]
    pub fn draw_buffer(&mut self, ui: &Ui, window_title: Option<&str>, buffer: &mut [u8]) {
        assert!(
            buffer.len() as WAddr == (self.addr_range.end - self.addr_range.start) as WAddr + 1
        );
        let base_addr = self.addr_range.start;
        self.draw_callbacks(
            ui,
            window_title,
            buffer,
            move |buffer, addr| Some(buffer[(addr - base_addr) as usize]),
            move |buffer, addr, value| buffer[(addr - base_addr) as usize] = value,
        );
    }

    pub fn draw_callbacks<T: ?Sized>(
        &mut self,
        ui: &Ui,
        window_title: Option<&str>,
        cb_data: &mut T,
        mut read: impl FnMut(&mut T, Addr) -> Option<u8>,
        mut write: impl FnMut(&mut T, Addr, u8),
    ) {
        self.compute_layout(ui);

        let window_token = if let Some(window_title) = window_title {
            let layout = self.layout.as_ref().unwrap();
            let token = if let Some(token) = ui
                .window(window_title)
                .size_constraints([layout.win_width, -1.0], [layout.win_width, -1.0])
                .begin()
            {
                token
            } else {
                return;
            };
            self.handle_options_right_click(ui);
            Some(token)
        } else {
            None
        };

        self.highlights.sort_unstable_by_key(|h| h.0.start);

        let layout = self.layout.as_ref().unwrap();
        let mut invalidate_layout = false;

        let frame_padding = ui.push_style_var(StyleVar::FramePadding([0.0; 2]));
        let item_spacing = ui.push_style_var(StyleVar::ItemSpacing([0.0; 2]));

        ui.child_window("##data")
            .movable(false)
            .no_nav()
            .scroll_bar(false)
            .focused(self.selected_addr_changed)
            .size([
                layout.win_width,
                ui.content_region_avail()[1] - layout.data_preview_and_footer_height,
            ])
            .build(|| {
                let layout = self.layout.as_ref().unwrap();

                let win_height_int = YPos::from(ui.window_size()[1]);
                let scroll_max_int = layout.data_height_int - win_height_int;

                self.scrollbar.scroll = if ui.is_window_hovered() {
                    self.scrollbar.scroll.as_signed()
                        - SignedYPos::from(ui.io().mouse_wheel * 3.0)
                            * layout.data_row_height_with_spacing_int.as_signed()
                } else {
                    self.scrollbar.scroll.as_signed()
                }
                .clamp(SignedYPos(0), scroll_max_int.as_signed())
                .as_unsigned();

                {
                    let mut new_addr = None;
                    if ui.is_window_focused() {
                        if ui.is_key_pressed(Key::UpArrow) {
                            new_addr = Some(self.selected_addr.saturating_sub(self.bytes_per_row));
                        }
                        if ui.is_key_pressed(Key::DownArrow) {
                            new_addr = Some(self.selected_addr.saturating_add(self.bytes_per_row));
                        }
                        let col_size = self.col_size.get() as Addr;
                        if ui.is_key_pressed(Key::LeftArrow) {
                            new_addr = Some(if self.flags.contains(Flags::LITTLE_ENDIAN_COLS) {
                                if self.selected_addr % col_size == col_size - 1 {
                                    self.selected_addr.saturating_sub((col_size << 1) - 1)
                                } else {
                                    self.selected_addr.saturating_add(1)
                                }
                            } else {
                                self.selected_addr.saturating_sub(1)
                            });
                        }
                        if ui.is_key_pressed(Key::RightArrow) {
                            new_addr = Some(if self.flags.contains(Flags::LITTLE_ENDIAN_COLS) {
                                if self.selected_addr % col_size == 0 {
                                    self.selected_addr.saturating_add((col_size << 1) - 1)
                                } else {
                                    self.selected_addr.saturating_sub(1)
                                }
                            } else {
                                self.selected_addr.saturating_add(1)
                            });
                        }
                    }
                    if let Some(new_addr) = new_addr {
                        self.set_selected_addr(new_addr);
                    }
                }

                let layout = self.layout.as_ref().unwrap();

                let win_pos = ui.window_pos();
                let mouse_pos = ui.io().mouse_pos;

                let hex_start_screen_x = win_pos[0] + layout.hex_start_win_x;
                let ascii_start_screen_x = win_pos[0] + layout.ascii_start_win_x;

                if ui.is_window_hovered() && ui.is_mouse_clicked(MouseButton::Left) {
                    let scroll_offset_y =
                        f32::from(self.scrollbar.scroll % layout.data_row_height_with_spacing_int);
                    let hex_end_screen_x = layout.hex_end_win_x + win_pos[0];
                    let ascii_end_screen_x = layout.ascii_end_scrollbar_start_win_x + win_pos[0];
                    let row_base = (((mouse_pos[1] - win_pos[1] + scroll_offset_y)
                        / layout.data_row_height_with_spacing)
                        as WAddr
                        + self
                            .scrollbar
                            .scroll
                            .div_into_int(layout.data_row_height_with_spacing_int)
                            as WAddr)
                        * self.bytes_per_row as WAddr;
                    if let Some((col, mut col_byte)) = if (hex_start_screen_x..hex_end_screen_x)
                        .contains(&mouse_pos[0])
                    {
                        let rel_x = mouse_pos[0] - hex_start_screen_x;
                        let col = ((rel_x + layout.item_spacing_x * 0.5)
                            / layout.hex_col_width_with_spacing)
                            .min(self.cols.get() as f32) as WAddr;
                        Some((
                            col,
                            ((rel_x - col as f32 * layout.hex_col_width_with_spacing)
                                / layout.hex_byte_width)
                                .clamp(0.0, (self.col_size.get() - 1) as f32)
                                as WAddr,
                        ))
                    } else if (ascii_start_screen_x..ascii_end_screen_x).contains(&mouse_pos[0]) {
                        let rel_x = mouse_pos[0] - ascii_start_screen_x;
                        let col = ((rel_x + layout.item_spacing_x * 0.5)
                            / layout.ascii_col_width_with_spacing)
                            .min(self.cols.get() as f32) as WAddr;
                        Some((
                            col,
                            ((rel_x - col as f32 * layout.ascii_col_width_with_spacing)
                                / layout.glyph_width)
                                .clamp(0.0, (self.col_size.get() - 1) as f32)
                                as WAddr,
                        ))
                    } else {
                        None
                    } {
                        if self.flags.contains(Flags::LITTLE_ENDIAN_COLS) {
                            col_byte = self.col_size.get() as WAddr - 1 - col_byte;
                        }
                        self.set_selected_addr(
                            (row_base + col * self.col_size.get() as WAddr + col_byte)
                                .min(self.addr_range.end as WAddr)
                                as Addr,
                        );
                    }
                }

                if self.selected_addr_changed {
                    self.focus_on_selected_addr(ui);
                }

                let layout = self.layout.as_ref().unwrap();

                self.scrollbar.draw(
                    ui,
                    win_pos[0] + layout.ascii_end_scrollbar_start_win_x,
                    win_pos[1],
                    layout.scrollbar_size,
                    ui.window_size()[1],
                    mouse_pos,
                    (win_height_int / layout.data_height_int).into(),
                    scroll_max_int,
                );

                if self.flags.contains(Flags::SHOW_ASCII) {
                    {
                        let sep_screen_x = win_pos[0] + layout.ascii_sep_win_x;
                        let sep_start_y = win_pos[1];
                        ui.get_window_draw_list()
                            .add_line(
                                [sep_screen_x, sep_start_y],
                                [sep_screen_x, sep_start_y + ui.window_size()[1]],
                                ui.style_color(StyleColor::Border),
                            )
                            .build();
                    }
                }

                let scroll_offset_y_int =
                    self.scrollbar.scroll % layout.data_row_height_with_spacing_int;
                let scroll_offset_y = f32::from(scroll_offset_y_int);
                let content_start_y = win_pos[1] - scroll_offset_y;

                self.visible_data_rows = (
                    self.scrollbar
                        .scroll
                        .div_into_int(layout.data_row_height_with_spacing_int)
                        as Addr,
                    (self.scrollbar.scroll + scroll_offset_y_int + win_height_int)
                        .div_into_int(layout.data_row_height_with_spacing_int)
                        .min((layout.total_rows - 1) as YPosRaw) as Addr,
                )
                    .into();

                let mut cur_base_addr =
                    self.addr_range.start + self.visible_data_rows.start * self.bytes_per_row;

                let mut cur_highlight_color = None;
                let mut cur_highlight_i = self
                    .highlights
                    .iter()
                    .position(|h| h.0.end >= cur_base_addr)
                    .unwrap_or(self.highlights.len());

                for rel_row in 0..=self.visible_data_rows.end - self.visible_data_rows.start {
                    let row_start_screen_y =
                        content_start_y + rel_row as f32 * layout.data_row_height_with_spacing;

                    ui.set_cursor_screen_pos([win_pos[0], row_start_screen_y]);
                    ui.text(if self.flags.contains(Flags::UPPERCASE_HEX) {
                        str_buf!(
                            self.str_buffer,
                            "{:0addr_digits$X}:",
                            cur_base_addr,
                            addr_digits = layout.addr_digits.get() as usize
                        )
                    } else {
                        str_buf!(
                            self.str_buffer,
                            "{:0addr_digits$x}:",
                            cur_base_addr,
                            addr_digits = layout.addr_digits.get() as usize
                        )
                    });

                    for col_i in 0..self.cols.get() {
                        let hex_col_start_screen_x =
                            hex_start_screen_x + col_i as f32 * layout.hex_col_width_with_spacing;
                        let ascii_col_start_screen_x = ascii_start_screen_x
                            + col_i as f32 * layout.ascii_col_width_with_spacing;

                        let col_base_addr = if self.flags.contains(Flags::LITTLE_ENDIAN_COLS) {
                            cur_base_addr += self.col_size.get() as Addr;
                            cur_base_addr - 1
                        } else {
                            let addr = cur_base_addr;
                            cur_base_addr = cur_base_addr.wrapping_add(self.col_size.get() as Addr);
                            addr
                        };

                        for byte_i in 0..self.col_size.get() {
                            let addr = if self.flags.contains(Flags::LITTLE_ENDIAN_COLS) {
                                col_base_addr - byte_i as Addr
                            } else {
                                col_base_addr + byte_i as Addr
                            };

                            let hex_byte_start_screen_x =
                                hex_col_start_screen_x + byte_i as f32 * layout.hex_byte_width;
                            let ascii_byte_start_screen_x =
                                ascii_col_start_screen_x + byte_i as f32 * layout.glyph_width;

                            if let Some(color) = if self.selected_addr == addr {
                                Some(ui.style_color(StyleColor::TextSelectedBg))
                            } else {
                                if cur_highlight_color.is_some()
                                    && !self.highlights[cur_highlight_i].0.contains(&addr)
                                {
                                    cur_highlight_color = None;
                                }
                                if cur_highlight_color.is_none() {
                                    while cur_highlight_i < self.highlights.len() {
                                        let highlight = &self.highlights[cur_highlight_i];
                                        if highlight.0.end >= addr {
                                            if highlight.0.start <= addr {
                                                cur_highlight_color = Some(highlight.1);
                                            }
                                            break;
                                        }
                                        cur_highlight_i += 1;
                                    }
                                }
                                cur_highlight_color
                            } {
                                let draw_list = ui.get_window_draw_list();
                                let half_item_spacing = layout.item_spacing_x * 0.5;
                                let half_row_spacing = layout.data_row_spacing * 0.5;

                                let mut spacing_left = if addr % self.col_size.get() as Addr == 0 {
                                    half_item_spacing
                                } else {
                                    0.0
                                };
                                let mut spacing_right = if addr % self.col_size.get() as Addr
                                    == (self.col_size.get() - 1) as Addr
                                {
                                    half_item_spacing
                                } else {
                                    0.0
                                };
                                if self.flags.contains(Flags::LITTLE_ENDIAN_COLS) {
                                    core::mem::swap(&mut spacing_left, &mut spacing_right);
                                }

                                draw_list
                                    .add_rect(
                                        [
                                            hex_byte_start_screen_x - spacing_left,
                                            row_start_screen_y - half_row_spacing,
                                        ],
                                        [
                                            hex_byte_start_screen_x
                                                + layout.hex_byte_width
                                                + spacing_right,
                                            row_start_screen_y
                                                + layout.data_row_height
                                                + half_row_spacing,
                                        ],
                                        color,
                                    )
                                    .filled(true)
                                    .build();

                                if self.flags.contains(Flags::SHOW_ASCII) {
                                    draw_list
                                        .add_rect(
                                            [
                                                ascii_byte_start_screen_x - spacing_left,
                                                row_start_screen_y - half_row_spacing,
                                            ],
                                            [
                                                ascii_byte_start_screen_x
                                                    + layout.glyph_width
                                                    + spacing_right,
                                                row_start_screen_y
                                                    + layout.data_row_height
                                                    + half_row_spacing,
                                            ],
                                            color,
                                        )
                                        .filled(true)
                                        .build();
                                }
                            }

                            if let Some(data) = read(cb_data, addr) {
                                let text_color = ui.style_color(
                                    if self.flags.contains(Flags::GRAY_OUT_ZEROS) && data == 0 {
                                        StyleColor::TextDisabled
                                    } else {
                                        StyleColor::Text
                                    },
                                );

                                ui.set_cursor_screen_pos([
                                    hex_byte_start_screen_x,
                                    row_start_screen_y,
                                ]);
                                ui.text_colored(
                                    text_color,
                                    if self.flags.contains(Flags::SHOW_HEXII) {
                                        if (0x20..0x7F).contains(&data) {
                                            str_buf!(self.str_buffer, ".{}", data as char)
                                        } else if data == 0 {
                                            str_buf!(self.str_buffer, "  ")
                                        } else if data == 0xFF {
                                            str_buf!(self.str_buffer, "##")
                                        } else if self.flags.contains(Flags::UPPERCASE_HEX) {
                                            str_buf!(self.str_buffer, "{:02X}", data)
                                        } else {
                                            str_buf!(self.str_buffer, "{:02x}", data)
                                        }
                                    } else if self.flags.contains(Flags::UPPERCASE_HEX) {
                                        str_buf!(self.str_buffer, "{:02X}", data)
                                    } else {
                                        str_buf!(self.str_buffer, "{:02x}", data)
                                    },
                                );

                                if self.flags.contains(Flags::SHOW_ASCII) {
                                    ui.set_cursor_screen_pos([
                                        ascii_byte_start_screen_x,
                                        row_start_screen_y,
                                    ]);
                                    ui.text_colored(
                                        text_color,
                                        str_buf!(
                                            self.str_buffer,
                                            "{}",
                                            if (0x20..0x7F).contains(&data) {
                                                data as char
                                            } else {
                                                '.'
                                            }
                                        ),
                                    );
                                }
                            } else {
                                continue;
                            }
                        }
                    }
                }
            });

        drop((item_spacing, frame_padding));

        ui.spacing();
        ui.separator();

        // Data preview

        let layout = self.layout.as_ref().unwrap();
        if self.flags.contains(Flags::SHOW_DATA_PREVIEW) {
            ui.child_window("##data_preview")
                .size([
                    layout.win_width,
                    ui.content_region_avail()[1] - layout.footer_height,
                ])
                .build(|| {
                    static DATA_PREVIEW_BASE_TYS: [DataPreviewBaseTy; 11] = [
                        DataPreviewBaseTy::I8,
                        DataPreviewBaseTy::I16,
                        DataPreviewBaseTy::I24,
                        DataPreviewBaseTy::I32,
                        DataPreviewBaseTy::I64,
                        DataPreviewBaseTy::I128,
                        DataPreviewBaseTy::F32,
                        DataPreviewBaseTy::F64,
                        DataPreviewBaseTy::C8,
                        DataPreviewBaseTy::C16,
                        DataPreviewBaseTy::C32,
                    ];

                    ui.align_text_to_frame_padding();
                    ui.text("Preview as:");

                    ui.same_line();
                    let mut ty_index = self.data_preview_ty.base as usize;
                    ui.set_next_item_width(
                        ui.calc_text_size("0")[0] * 10.0
                            + unsafe { ui.style().frame_padding[0] } * 2.0,
                    );
                    if ui.combo_simple_string(
                        "##data_preview_base_ty",
                        &mut ty_index,
                        &DATA_PREVIEW_BASE_TYS,
                    ) {
                        invalidate_layout = true;
                        self.data_preview_ty.base = DATA_PREVIEW_BASE_TYS[ty_index];
                    }

                    if self.data_preview_ty.base.allows_signed() {
                        ui.same_line();
                        ui.checkbox("Signed", &mut self.data_preview_ty.signed);
                    }

                    if self.data_preview_ty.base.allows_endianness() {
                        ui.same_line();
                        ui.checkbox("Little endian", &mut self.data_preview_ty.little_endian);
                    }

                    macro_rules! preview_int {
                        ($bytes: literal) => {{
                            let mut value = 0_u128;
                            let mut valid = true;
                            let mut addr = self.selected_addr;
                            let mut prev_addr = None;
                            if self.data_preview_ty.little_endian || $bytes == 1 {
                                for i in 0..$bytes {
                                    if prev_addr == Some(addr) {
                                        valid = false;
                                        break;
                                    }
                                    if let Some(byte) = read(cb_data, addr) {
                                        value |= (byte as u128) << (i << 3);
                                    } else {
                                        valid = false;
                                        break;
                                    }
                                    prev_addr = Some(addr);
                                    addr = addr
                                        .saturating_add(1)
                                        .clamp(self.addr_range.start, self.addr_range.end);
                                }
                            } else {
                                for i in 0..$bytes {
                                    if prev_addr == Some(addr) {
                                        valid = false;
                                        break;
                                    }
                                    if let Some(byte) = read(cb_data, addr) {
                                        value |= (byte as u128) << (($bytes - 1 - i) << 3);
                                    } else {
                                        valid = false;
                                        break;
                                    }
                                    prev_addr = Some(addr);
                                    addr = addr
                                        .saturating_add(1)
                                        .clamp(self.addr_range.start, self.addr_range.end);
                                }
                            }

                            ui.text("Dec:");
                            ui.same_line();
                            if valid {
                                if self.data_preview_ty.signed {
                                    ui.text(&format!(
                                        "{}",
                                        (value as i128) << (128 - ($bytes << 3))
                                            >> (128 - ($bytes << 3))
                                    ));
                                } else {
                                    ui.text(&format!("{}", value));
                                }
                            } else {
                                ui.text("N/A");
                            }

                            ui.text("Hex:");
                            ui.same_line();
                            if valid {
                                self.str_buffer.clear();
                                for (j, (i, byte)) in value
                                    .to_be_bytes()
                                    .iter()
                                    .enumerate()
                                    .skip(16 - $bytes)
                                    .enumerate()
                                {
                                    if i & 1 == 0 && j != 0 {
                                        self.str_buffer.push(' ');
                                    }
                                    let _ = write!(self.str_buffer, "{:02X}", byte);
                                }
                                ui.text(&self.str_buffer);
                            } else {
                                ui.text("N/A");
                            }

                            ui.text("Bin:");
                            ui.same_line();
                            if valid {
                                self.str_buffer.clear();
                                for byte in &value.to_be_bytes()[16 - $bytes..] {
                                    let _ = write!(self.str_buffer, "{:08b}", byte);
                                    self.str_buffer.push(' ');
                                }
                                ui.text(&self.str_buffer);
                            } else {
                                ui.text("N/A");
                            }
                        }};
                    }

                    macro_rules! read_bytes {
                        ($bytes: expr) => {{
                            let mut value = [0; $bytes];
                            let mut valid = true;
                            let mut addr = self.selected_addr;
                            let mut prev_addr = None;
                            if self.data_preview_ty.little_endian {
                                for i in 0..$bytes {
                                    if prev_addr == Some(addr) {
                                        valid = false;
                                        break;
                                    }
                                    if let Some(byte) = read(cb_data, addr) {
                                        value[$bytes - 1 - i] = byte;
                                    } else {
                                        valid = false;
                                        break;
                                    }
                                    prev_addr = Some(addr);
                                    addr = addr
                                        .saturating_add(1)
                                        .clamp(self.addr_range.start, self.addr_range.end);
                                }
                            } else {
                                for i in 0..$bytes {
                                    if prev_addr == Some(addr) {
                                        valid = false;
                                        break;
                                    }
                                    if let Some(byte) = read(cb_data, addr) {
                                        value[i] = byte;
                                    } else {
                                        valid = false;
                                        break;
                                    }
                                    prev_addr = Some(addr);
                                    addr = addr
                                        .saturating_add(1)
                                        .clamp(self.addr_range.start, self.addr_range.end);
                                }
                            }
                            (value, valid)
                        }};
                    }

                    macro_rules! preview_float {
                        ($ty: ty) => {{
                            let (bytes, valid) = read_bytes!(size_of::<$ty>());

                            ui.text("Value:");
                            ui.same_line();
                            if valid {
                                ui.text(str_buf!(
                                    self.str_buffer,
                                    "{}",
                                    <$ty>::from_le_bytes(bytes)
                                ));
                            } else {
                                ui.text("N/A");
                            }
                        }};
                    }

                    macro_rules! preview_char {
                        (
                            $bytes: expr,
                            |$bytes_ident: ident| $value: expr, |$value_ident: ident| $char: expr
                        ) => {{
                            let ($bytes_ident, valid) = read_bytes!($bytes);

                            ui.text("Value:");
                            ui.same_line();
                            if valid {
                                let $value_ident = $value;
                                if let Some(char) = $char {
                                    ui.text(str_buf!(
                                        self.str_buffer,
                                        "\"{}\"",
                                        char.escape_debug()
                                    ));
                                } else {
                                    ui.text(str_buf!(self.str_buffer, "\\u{{{:x}}}", $value_ident));
                                }
                            } else {
                                ui.text("N/A");
                            }
                        }};
                    }

                    match self.data_preview_ty.base {
                        DataPreviewBaseTy::I8 => preview_int!(1),
                        DataPreviewBaseTy::I16 => preview_int!(2),
                        DataPreviewBaseTy::I24 => preview_int!(3),
                        DataPreviewBaseTy::I32 => preview_int!(4),
                        DataPreviewBaseTy::I64 => preview_int!(8),
                        DataPreviewBaseTy::I128 => preview_int!(16),
                        DataPreviewBaseTy::F32 => preview_float!(f32),
                        DataPreviewBaseTy::F64 => preview_float!(f64),
                        DataPreviewBaseTy::C8 => {
                            preview_char!(1, |bytes| bytes[0], |value| Some(char::from(value)));
                        }
                        DataPreviewBaseTy::C16 => {
                            preview_char!(2, |bytes| u16::from_be_bytes(bytes), |value| {
                                char::decode_utf16(once(value)).next().unwrap().ok()
                            });
                        }
                        DataPreviewBaseTy::C32 => {
                            preview_char!(4, |bytes| u32::from_be_bytes(bytes), |value| {
                                char::from_u32(u32::from_be_bytes(bytes))
                            });
                        }
                    }
                });

            ui.separator();
        }

        // Options

        if self.flags.contains(Flags::SHOW_VIEW_OPTIONS) {
            if ui.button("Options...") {
                ui.open_popup("options");
            }

            ui.popup("options", || {
                let mut cols = self.cols.get();
                if Drag::new("##cols")
                    .display_format("Cols: %d")
                    .build(ui, &mut cols)
                {
                    invalidate_layout = true;
                    self.cols = NonZeroU8::new(cols.max(1)).unwrap();
                    self.bytes_per_row = self.calc_bytes_per_row();
                }

                ui.same_line();

                let mut col_size = self.col_size.get();
                if Drag::new("##col_size")
                    .display_format("Col size: %d")
                    .build(ui, &mut col_size)
                {
                    invalidate_layout = true;
                    self.col_size = NonZeroU8::new(col_size.max(1)).unwrap();
                    self.bytes_per_row = self.calc_bytes_per_row();
                }

                ui.checkbox_flags("Gray out zeros", &mut self.flags, Flags::GRAY_OUT_ZEROS);
                ui.checkbox_flags("Uppercase hex", &mut self.flags, Flags::UPPERCASE_HEX);
                ui.checkbox_flags(
                    "Little endian cols",
                    &mut self.flags,
                    Flags::LITTLE_ENDIAN_COLS,
                );
                ui.checkbox_flags("Show HexII", &mut self.flags, Flags::SHOW_HEXII);
                invalidate_layout |=
                    ui.checkbox_flags("Show ASCII", &mut self.flags, Flags::SHOW_ASCII);
                invalidate_layout |= ui.checkbox_flags(
                    "Show data preview",
                    &mut self.flags,
                    Flags::SHOW_DATA_PREVIEW,
                );
            });
            ui.same_line();
        }

        let layout = self.layout.as_ref().unwrap();

        // Address range bounds

        if self.flags.contains(Flags::SHOW_RANGE) {
            if ui.content_region_avail()[0] < layout.range_width {
                ui.new_line();
            }
            ui.text(if self.flags.contains(Flags::UPPERCASE_HEX) {
                str_buf!(
                    self.str_buffer,
                    "Range {:0addr_digits$X}..{:0addr_digits$X}",
                    self.addr_range.start,
                    self.addr_range.end,
                    addr_digits = layout.addr_digits.get() as usize
                )
            } else {
                str_buf!(
                    self.str_buffer,
                    "Range {:0addr_digits$x}..{:0addr_digits$x}",
                    self.addr_range.start,
                    self.addr_range.end,
                    addr_digits = layout.addr_digits.get() as usize
                )
            });
            ui.same_line();
        }

        // Address input field

        if ui.content_region_avail()[0] < layout.addr_input_width {
            ui.new_line();
        }
        ui.set_next_item_width(layout.addr_input_width);

        if self.selected_addr_changed {
            self.addr_input_buffer.clear();
            write!(
                self.addr_input_buffer,
                "{:0addr_digits$X}",
                self.selected_addr,
                addr_digits = layout.addr_digits.get() as usize
            )
            .unwrap();
        }

        self.selected_addr_changed = false;
        if ui
            .input_text("##address", &mut self.addr_input_buffer)
            .auto_select_all(true)
            .chars_hexadecimal(true)
            .enter_returns_true(true)
            .no_horizontal_scroll(true)
            .build()
        {
            if let Ok(addr) = Addr::from_str_radix(&self.addr_input_buffer, 16) {
                self.set_selected_addr(addr);
            }
        };

        // Finalize

        if invalidate_layout {
            self.layout = None;
        }

        if let Some(token) = window_token {
            token.end();
        }
    }
}
