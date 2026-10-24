#[cfg(any(windows, test))]
pub mod activity;
pub mod activity_buffer;
pub mod dpapi;
pub mod taskbar;
pub mod toast;

#[cfg(windows)]
mod apartment {
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::System::WinRT::{RO_INIT_SINGLETHREADED, RoInitialize, RoUninitialize};

    // Apartment initialization must be balanced on the initializing thread.
    pub(crate) struct Apartment {
        owned: bool,
        _thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
    }

    impl Apartment {
        pub(crate) fn enter() -> windows::core::Result<Self> {
            // SAFETY: no pointers are supplied. An existing apartment is reused;
            // only a successful initialization is balanced by this guard.
            let owned = match unsafe { RoInitialize(RO_INIT_SINGLETHREADED) } {
                Ok(()) => true,
                Err(error) if error.code() == RPC_E_CHANGED_MODE => false,
                Err(error) => return Err(error),
            };
            Ok(Self {
                owned,
                _thread_bound: std::marker::PhantomData,
            })
        }
    }

    impl Drop for Apartment {
        fn drop(&mut self) {
            if self.owned {
                // SAFETY: this non-Send guard owns a successful RoInitialize call.
                unsafe { RoUninitialize() };
            }
        }
    }
}
