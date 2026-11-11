//! GPUI 0.2 does not dispatch macOS magnify events. Observe only this app's
//! native pinch events and relay them to the UI task; never intercept other input.
use super::*;

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs, deprecated)]
mod native {
    use block::ConcreteBlock;
    use cocoa::foundation::{NSPoint, NSRect};
    use objc::{class, msg_send, runtime::Object, sel, sel_impl};
    use std::{marker::PhantomData, rc::Rc};

    pub(super) struct Gesture {
        pub position: (f32, f32),
        pub magnification: f32,
    }
    pub struct Monitor {
        token: *mut Object,
        // AppKit installs and removes local event monitors on the main thread.
        _main_thread: PhantomData<Rc<()>>,
    }
    impl Monitor {
        pub(super) fn new() -> (Self, flume::Receiver<Gesture>) {
            let (tx, rx) = flume::unbounded();
            let callback = ConcreteBlock::new(move |event: *mut Object| -> *mut Object {
                // SAFETY: AppKit invokes the copied block on the main thread with
                // a live NSEvent. This monitor's mask includes only magnify events.
                unsafe {
                    let window: *mut Object = msg_send![event, window];
                    if !window.is_null() {
                        let key: bool = msg_send![window, isKeyWindow];
                        let view: *mut Object = msg_send![window, contentView];
                        if key && !view.is_null() {
                            let point: NSPoint = msg_send![event, locationInWindow];
                            let frame: NSRect = msg_send![view, frame];
                            let magnification: f64 = msg_send![event, magnification];
                            let _ = tx.send(Gesture {
                                position: (point.x as f32, (frame.size.height - point.y) as f32),
                                magnification: magnification as f32,
                            });
                        }
                    }
                }
                event
            })
            .copy();
            // SAFETY: AppKit copies the heap block and owns it until removal.
            // Retain the monitor token for the lifetime of this main-thread guard.
            let token: *mut Object = unsafe {
                let token: *mut Object = msg_send![class!(NSEvent),
                    addLocalMonitorForEventsMatchingMask: (1u64 << 30)
                    handler: &*callback];
                if !token.is_null() {
                    let _: () = msg_send![token, retain];
                }
                token
            };
            (
                Self {
                    token,
                    _main_thread: PhantomData,
                },
                rx,
            )
        }
    }
    impl Drop for Monitor {
        fn drop(&mut self) {
            // SAFETY: The guard is main-thread-only and retains a valid token.
            if !self.token.is_null() {
                unsafe {
                    let _: () = msg_send![class!(NSEvent), removeMonitor: self.token];
                    let _: () = msg_send![self.token, release];
                }
            }
        }
    }
}
#[cfg(target_os = "macos")]
pub(super) use native::Monitor;
#[cfg(not(target_os = "macos"))]
pub(super) struct Monitor;

pub(super) fn install(cx: &mut Context<Workspace>) -> Monitor {
    #[cfg(target_os = "macos")]
    {
        let (monitor, rx) = native::Monitor::new();
        cx.spawn(async move |this, cx| {
            while let Ok(gesture) = rx.recv_async().await {
                if this
                    .update(cx, |this, cx| {
                        if this.filter_builder.is_some()
                            || this.form.show_connection_modal
                            || this.show_about_dialog
                            || this.object_search.is_some()
                            || this.definition_search.is_some()
                            || this.script_chooser.is_some()
                            || this.script_action.is_some()
                            || this.updates.read(cx).is_open()
                        {
                            return;
                        }
                        if let Some(tab) = this.tabs.get_mut(this.active_tab)
                            && tab.active_subtab == TableSubTab::Diagram
                        {
                            let position = point(px(gesture.position.0), px(gesture.position.1));
                            let bounds = tab.diagram.scroll.bounds();
                            if bounds.contains(&position) && gesture.magnification.is_finite() {
                                tab.diagram.zoom_at(
                                    tab.diagram.zoom * (1.0 + gesture.magnification).max(0.1),
                                    position - bounds.origin,
                                );
                                cx.notify();
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        monitor
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = cx;
        Monitor
    }
}
