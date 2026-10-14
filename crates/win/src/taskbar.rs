#[cfg(any(windows, test))]
fn badge_label(count: u32) -> Option<String> {
    match count {
        0 => None,
        1..=9 => Some(count.to_string()),
        _ => Some("9+".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::badge_label;

    #[test]
    fn badge_zero_clears_and_counts_cap_at_nine_plus() {
        assert_eq!(badge_label(0), None);
        assert_eq!(badge_label(1).as_deref(), Some("1"));
        assert_eq!(badge_label(9).as_deref(), Some("9"));
        assert_eq!(badge_label(10).as_deref(), Some("9+"));
        assert_eq!(badge_label(u32::MAX).as_deref(), Some("9+"));
    }
}

#[cfg(windows)]
pub use native::{flash, set_overlay};

#[cfg(windows)]
mod native {
    use windows::Win32::Foundation::{COLORREF, E_FAIL, HWND};
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
    use windows::Win32::UI::Shell::{ITaskbarList3, TaskbarList};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateIconIndirect, DestroyIcon, FLASHW_TIMERNOFG, FLASHW_TRAY, FLASHWINFO, FlashWindowEx,
        GetForegroundWindow, HICON, ICONINFO,
    };
    use windows::core::{Error, HSTRING, Result, w};

    fn failed(operation: &str) -> Error {
        // Many GDI APIs do not set last-error, so do not report stale errors.
        Error::new(E_FAIL, operation)
    }

    struct Drawing {
        dc: HDC,
        objects: Vec<HGDIOBJ>,
    }

    impl Drawing {
        fn new() -> Result<Self> {
            // SAFETY: creates a private memory DC without borrowing another DC.
            let dc = unsafe { CreateCompatibleDC(None) };
            if dc.is_invalid() {
                return Err(failed("CreateCompatibleDC"));
            }
            Ok(Self {
                dc,
                objects: Vec::new(),
            })
        }

        fn own(&mut self, object: HGDIOBJ) -> Result<HGDIOBJ> {
            if object.is_invalid() {
                return Err(failed("GDI object creation"));
            }
            self.objects.push(object);
            Ok(object)
        }

        fn release_dc(&mut self) -> Result<()> {
            if !self.dc.is_invalid() {
                // SAFETY: this DC is exclusively owned. Releasing it deselects
                // the bitmap before CreateIconIndirect, which requires that.
                unsafe { DeleteDC(self.dc).ok()? };
                self.dc = HDC::default();
            }
            Ok(())
        }

        fn select(&self, object: HGDIOBJ) -> Result<()> {
            // SAFETY: both handles remain alive until Drawing is dropped.
            let previous = unsafe { SelectObject(self.dc, object) };
            if previous.is_invalid() {
                return Err(failed("SelectObject"));
            }
            Ok(())
        }
    }

    impl Drop for Drawing {
        fn drop(&mut self) {
            // SAFETY: these private handles have exactly one owner. Deleting the
            // DC releases all selections before DeleteObject, including errors.
            unsafe {
                if !self.dc.is_invalid() {
                    let _ = DeleteDC(self.dc);
                }
                for object in self.objects.drain(..) {
                    let _ = DeleteObject(object);
                }
            }
        }
    }

    struct Icon(HICON);
    impl Drop for Icon {
        fn drop(&mut self) {
            // SAFETY: CreateIconIndirect returns an owned, non-shared icon.
            let _ = unsafe { DestroyIcon(self.0) };
        }
    }

    fn badge_icon(label: &str) -> Result<Icon> {
        let mut drawing = Drawing::new()?;
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: 16,
                biHeight: -16,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        // SAFETY: all pointer arguments reference live storage. The bitmap owns
        // a 16 * 16 * 4 byte buffer, accessed only while Drawing owns it.
        let color = unsafe {
            CreateDIBSection(Some(drawing.dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)?
        };
        let color_object = drawing.own(HGDIOBJ(color.0))?;
        if bits.is_null() {
            return Err(failed("CreateDIBSection pixel buffer"));
        }
        unsafe { std::ptr::write_bytes(bits.cast::<u8>(), 0, 16 * 16 * 4) };
        drawing.select(color_object)?;
        // SAFETY: creates private brush/font handles and borrows a stock pen.
        let brush = drawing.own(unsafe { HGDIOBJ(CreateSolidBrush(COLORREF(0x003b3bed)).0) })?;
        drawing.select(brush)?;
        drawing.select(unsafe { GetStockObject(NULL_PEN) })?;
        let font = drawing.own(unsafe {
            HGDIOBJ(
                CreateFontW(
                    if label.len() > 1 { -10 } else { -12 },
                    0,
                    0,
                    0,
                    FW_BOLD.0 as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET,
                    OUT_DEFAULT_PRECIS,
                    CLIP_DEFAULT_PRECIS,
                    NONANTIALIASED_QUALITY,
                    u32::from(DEFAULT_PITCH.0 | FF_SWISS.0),
                    w!("Segoe UI"),
                )
                .0,
            )
        })?;
        drawing.select(font)?;
        let text: Vec<u16> = label.encode_utf16().collect();
        // SAFETY: the private DC and selected objects are alive; TextOutW's slice
        // remains valid throughout the synchronous call.
        unsafe {
            Ellipse(drawing.dc, 0, 0, 16, 16).ok()?;
            if SetBkMode(drawing.dc, TRANSPARENT) == 0 {
                return Err(failed("SetBkMode"));
            }
            if SetTextColor(drawing.dc, COLORREF(0x00ffffff)).0 == CLR_INVALID {
                return Err(failed("SetTextColor"));
            }
            if SetTextAlign(drawing.dc, TA_CENTER | TA_TOP) == GDI_ERROR as u32 {
                return Err(failed("SetTextAlign"));
            }
            TextOutW(drawing.dc, 8, 1, &text).ok()?;
            // Finish GDI writes before touching the DIB from the CPU.
            GdiFlush().ok()?;
            for pixel in std::slice::from_raw_parts_mut(bits.cast::<u32>(), 16 * 16) {
                // GDI clears alpha: restore opaque circle/text and clear corners.
                *pixel = if *pixel & 0x00ffffff == 0 {
                    0
                } else {
                    *pixel | 0xff000000
                };
            }
        }
        drawing.release_dc()?;
        // A monochrome scanline is WORD aligned: 2 bytes * 16 rows.
        let mask_bits = [0u8; 32];
        let mask = unsafe { CreateBitmap(16, 16, 1, 1, Some(mask_bits.as_ptr().cast())) };
        drawing.own(HGDIOBJ(mask.0))?;
        // SAFETY: CreateIconIndirect copies both bitmaps; their owner stays alive
        // through this call. The returned icon is independent of Drawing.
        let icon = unsafe {
            CreateIconIndirect(&ICONINFO {
                fIcon: true.into(),
                xHotspot: 0,
                yHotspot: 0,
                hbmMask: mask,
                hbmColor: color,
            })?
        };
        Ok(Icon(icon))
    }

    pub fn set_overlay(hwnd: HWND, count: u32) -> Result<()> {
        // Declared first so COM interfaces drop before apartment uninitialization.
        let _apartment = crate::apartment::Apartment::enter()?;
        // SAFETY: apartment initialized, no aggregation, expected interface IID.
        let taskbar: ITaskbarList3 =
            unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER)? };
        unsafe { taskbar.HrInit()? };
        let icon = super::badge_label(count)
            .map(|label| badge_icon(&label))
            .transpose()?;
        let description = HSTRING::from(format!("입력 대기 {count}개"));
        // SAFETY: HWND is supplied by the caller, icon and string live through
        // the call. SetOverlayIcon copies the icon, so it may be destroyed after.
        unsafe {
            taskbar.SetOverlayIcon(
                hwnd,
                icon.as_ref().map_or(HICON::default(), |icon| icon.0),
                &description,
            )
        }
    }

    pub fn flash(hwnd: HWND) {
        // SAFETY: these APIs inspect the supplied window handle and retain no
        // pointers. An invalid/stale handle fails without dereferencing Rust data.
        unsafe {
            if GetForegroundWindow() == hwnd {
                return;
            }
            let _ = FlashWindowEx(&FLASHWINFO {
                cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
                hwnd,
                dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG,
                uCount: u32::MAX,
                dwTimeout: 0,
            });
        }
    }
}
