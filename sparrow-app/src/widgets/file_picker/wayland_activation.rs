//! Wayland requires a serial from the triggering seat to raise an existing window.
use std::{
    cell::RefCell,
    ptr::NonNull,
    sync::Arc,
    time::{Duration, Instant},
};

use floem::{
    View, ViewId,
    action::{TimerToken, exec_after},
    context::PaintCx,
    views::Decorators,
    window::WindowId,
};
use raw_window_handle::{
    HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use wayland_backend::client::{Backend, ObjectId};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum, delegate_noop,
    globals::{GlobalList, GlobalListContents, registry_queue_init},
    protocol::{
        wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_surface::WlSurface,
    },
};
use wayland_protocols::xdg::activation::v1::client::{
    xdg_activation_token_v1::{self, XdgActivationTokenV1},
    xdg_activation_v1::XdgActivationV1,
};

const POLL_INTERVAL: Duration = Duration::from_millis(32);
const INPUT_LIFETIME: Duration = Duration::from_secs(2);

type NativeSurface = Box<dyn Fn() -> Option<NonNull<std::ffi::c_void>>>;

struct Owner {
    id: WindowId,
    display: NonNull<std::ffi::c_void>,
    surface: NativeSurface,
}

struct SurfaceView {
    id: ViewId,
    owner: bool,
    registered: Option<WindowId>,
}

thread_local! {
    static OWNER: RefCell<Option<Owner>> = const { RefCell::new(None) };
    static PICKER: RefCell<Option<(WindowId, NativeSurface)>> = const { RefCell::new(None) };
    static BROKER: RefCell<Option<Broker>> = const { RefCell::new(None) };
}

impl View for SurfaceView {
    fn id(&self) -> ViewId {
        self.id
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let Some(window) = &cx.window else { return };
        if self.registered.is_some() {
            return;
        }
        let Ok(display) = window.display_handle() else {
            return;
        };
        let RawDisplayHandle::Wayland(display) = display.as_raw() else {
            return;
        };
        let id = window.id();
        let weak = Arc::downgrade(window);
        let surface: NativeSurface = Box::new(move || {
            let strong = weak.upgrade()?;
            match strong.window_handle().ok()?.as_raw() {
                RawWindowHandle::Wayland(handle) => Some(handle.surface),
                _ => None,
            }
        });
        if self.owner {
            OWNER.replace(Some(Owner {
                id,
                display: display.display,
                surface,
            }));
        } else {
            PICKER.replace(Some((id, surface)));
        }
        self.registered = Some(id);
    }
}

impl Drop for SurfaceView {
    fn drop(&mut self) {
        let Some(id) = self.registered else { return };
        if !self.owner {
            let _ = PICKER.try_with(|entry| {
                if entry.borrow().as_ref().is_some_and(|entry| entry.0 == id) {
                    entry.take();
                }
            });
        }
    }
}

pub(crate) fn detach_owner(id: WindowId) {
    let is_owner = OWNER
        .with_borrow(|owner| owner.as_ref().is_some_and(|owner| owner.id == id));
    if is_owner {
        shutdown();
    }
}

pub(crate) fn shutdown() {
    BROKER.take();
    OWNER.take();
    PICKER.take();
}

pub(crate) fn observe_surface(owner: bool) -> impl View {
    SurfaceView {
        id: ViewId::new(),
        owner,
        registered: None,
    }
    .style(|s| s.absolute().size(1, 1).pointer_events_none())
}

struct Seat {
    seat: wl_seat::WlSeat,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer_surface: Option<WlSurface>,
    keyboard_surface: Option<WlSurface>,
}

struct Gesture {
    serial: u32,
    seat: wl_seat::WlSeat,
    surface: WlSurface,
    time: Instant,
}

#[derive(Default)]
struct State {
    seats: Vec<Seat>,
    gesture: Option<Gesture>,
    completed: Option<(ObjectId, String)>,
}

struct Broker {
    connection: Connection,
    queue: EventQueue<State>,
    globals: GlobalList,
    state: State,
    activation: XdgActivationV1,
    pending: Option<(WindowId, XdgActivationTokenV1, Instant)>,
    timer: Option<TimerToken>,
}

impl Broker {
    fn new(owner: &Owner) -> Option<Self> {
        (owner.surface)()?;
        // The existing Winit connection owns the display. This guest queue is
        // created and serviced on the same UI thread and dropped with the picker.
        let backend =
            unsafe { Backend::from_foreign_display(owner.display.as_ptr().cast()) };
        let connection = Connection::from_backend(backend);
        let (globals, queue) = registry_queue_init::<State>(&connection).ok()?;
        let handle = queue.handle();
        let activation = globals.bind(&handle, 1..=1, ()).ok()?;
        Some(Self {
            connection,
            queue,
            globals,
            state: State::default(),
            activation,
            pending: None,
            timer: None,
        })
    }

    fn listen(&mut self) {
        let handle = self.queue.handle();
        for global in self.globals.contents().clone_list() {
            if global.interface == "wl_seat" && global.version >= 5 {
                let seat = self.globals.registry().bind(global.name, 5, &handle, ());
                self.state.seats.push(Seat {
                    seat,
                    pointer: None,
                    keyboard: None,
                    pointer_surface: None,
                    keyboard_surface: None,
                });
            }
        }
        let _ = self.connection.flush();
    }

    fn surface(&self, pointer: NonNull<std::ffi::c_void>) -> Option<WlSurface> {
        // The pointer comes from a live Window's Wayland window handle. Both
        // surfaces use this guest connection's underlying Winit display.
        let id = unsafe {
            ObjectId::from_ptr(WlSurface::interface(), pointer.as_ptr().cast())
        }
        .ok()?;
        WlSurface::from_id(&self.connection, id).ok()
    }

    fn poll(&mut self) {
        if self.queue.dispatch_pending(&mut self.state).is_err() {
            return;
        }
        // A cancelled or superseded request does not get to raise a window.
        if let Some((completed, token)) = self.state.completed.take()
            && self
                .pending
                .as_ref()
                .is_some_and(|(_, current, _)| current.id() == completed)
            && let Some((id, _, _)) = self.pending.take()
        {
            PICKER.with_borrow(|picker| {
                if let Some((picker_id, surface)) = picker
                    && *picker_id == id
                    && let Some(target) =
                        surface().and_then(|pointer| self.surface(pointer))
                {
                    self.activation.activate(token, &target);
                }
            });
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|(_, _, time)| time.elapsed() > INPUT_LIFETIME)
            && let Some((_, token, _)) = self.pending.take()
        {
            token.destroy();
        }
        let _ = self.connection.flush();
    }

    fn activate(&mut self, target: WindowId) -> bool {
        self.poll();
        if self.pending.is_some() {
            return true;
        }
        let owner_surface = OWNER
            .with_borrow(|owner| owner.as_ref().and_then(|owner| (owner.surface)()));
        let Some(gesture) = self.state.gesture.take().filter(|gesture| {
            gesture.time.elapsed() <= INPUT_LIFETIME
                && owner_surface.is_some_and(|owner| {
                    gesture.surface.id().as_ptr().cast() == owner.as_ptr()
                })
        }) else {
            return false;
        };
        let token = self
            .activation
            .get_activation_token(&self.queue.handle(), ());
        token.set_serial(gesture.serial, &gesture.seat);
        token.set_surface(&gesture.surface);
        token.commit();
        self.pending = Some((target, token, Instant::now()));
        self.connection.flush().is_ok()
    }
}

impl Broker {
    fn stop_listening(&mut self) {
        if let Some(timer) = self.timer.take() {
            timer.cancel();
        }
        if let Some((_, token, _)) = self.pending.take() {
            token.destroy();
        }
        for mut seat in std::mem::take(&mut self.state.seats) {
            seat.release_devices();
            if seat.seat.version() >= 5 {
                seat.seat.release();
            }
        }
        self.state.gesture = None;
        self.state.completed = None;
        let _ = self.connection.flush();
    }
}

impl Drop for Broker {
    fn drop(&mut self) {
        self.stop_listening();
        self.activation.destroy();
        let _ = self
            .connection
            .backend()
            .destroy_object(&self.globals.registry().id());
    }
}

impl Seat {
    fn release_devices(&mut self) {
        if let Some(pointer) =
            self.pointer.take().filter(|pointer| pointer.version() >= 3)
        {
            pointer.release();
        }
        if let Some(keyboard) = self
            .keyboard
            .take()
            .filter(|keyboard| keyboard.version() >= 3)
        {
            keyboard.release();
        }
    }
}

pub(super) fn start() {
    BROKER.with_borrow_mut(|broker| {
        if broker.is_none() {
            *broker =
                OWNER.with_borrow(|owner| owner.as_ref().and_then(Broker::new));
        }
        if let Some(broker) = broker {
            broker.listen();
        }
    });
    schedule_poll();
}

pub(super) fn stop() {
    BROKER.with_borrow_mut(|broker| {
        if let Some(broker) = broker {
            broker.stop_listening();
        }
    });
}

pub(super) fn activate(id: WindowId) -> bool {
    BROKER.with_borrow_mut(|broker| {
        broker.as_mut().is_some_and(|broker| broker.activate(id))
    })
}

fn schedule_poll() {
    BROKER.with_borrow_mut(|broker| {
        if let Some(broker) = broker
            && !broker.state.seats.is_empty()
        {
            broker.timer = Some(exec_after(POLL_INTERVAL, |_| {
                BROKER.with_borrow_mut(|broker| {
                    if let Some(broker) = broker {
                        broker.timer = None;
                        broker.poll();
                    }
                });
                schedule_poll();
            }));
        }
    });
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
delegate_noop!(State: ignore XdgActivationV1);

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        else {
            return;
        };
        let Some(seat) = state.seats.iter_mut().find(|seat| seat.seat == *proxy)
        else {
            return;
        };
        if capabilities.contains(wl_seat::Capability::Pointer)
            && seat.pointer.is_none()
        {
            seat.pointer = Some(proxy.get_pointer(queue, proxy.clone()));
        }
        if capabilities.contains(wl_seat::Capability::Keyboard)
            && seat.keyboard.is_none()
        {
            seat.keyboard = Some(proxy.get_keyboard(queue, proxy.clone()));
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, wl_seat::WlSeat> for State {
    fn event(
        state: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        proxy: &wl_seat::WlSeat,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(seat) = state.seats.iter_mut().find(|seat| seat.seat == *proxy)
        else {
            return;
        };
        match event {
            wl_pointer::Event::Enter { surface, .. } => {
                seat.pointer_surface = Some(surface)
            }
            wl_pointer::Event::Leave { .. } => seat.pointer_surface = None,
            wl_pointer::Event::Button {
                serial,
                state: WEnum::Value(wl_pointer::ButtonState::Pressed),
                ..
            } => {
                state.gesture = record(proxy, seat.pointer_surface.as_ref(), serial);
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, wl_seat::WlSeat> for State {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        proxy: &wl_seat::WlSeat,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(seat) = state.seats.iter_mut().find(|seat| seat.seat == *proxy)
        else {
            return;
        };
        match event {
            wl_keyboard::Event::Enter { surface, .. } => {
                seat.keyboard_surface = Some(surface)
            }
            wl_keyboard::Event::Leave { .. } => seat.keyboard_surface = None,
            wl_keyboard::Event::Key {
                serial,
                state: WEnum::Value(_),
                ..
            } => {
                state.gesture =
                    record(proxy, seat.keyboard_surface.as_ref(), serial);
            }
            _ => {}
        }
    }
}

fn record(
    seat: &wl_seat::WlSeat,
    surface: Option<&WlSurface>,
    serial: u32,
) -> Option<Gesture> {
    Some(Gesture {
        serial,
        seat: seat.clone(),
        surface: surface?.clone(),
        time: Instant::now(),
    })
}

impl Dispatch<XdgActivationTokenV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &XdgActivationTokenV1,
        event: xdg_activation_token_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let xdg_activation_token_v1::Event::Done { token } = event else {
            return;
        };
        state.completed = Some((proxy.id(), token));
        proxy.destroy();
    }
}
