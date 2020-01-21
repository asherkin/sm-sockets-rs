use async_std::net::UdpSocket;
use futures::executor::{LocalPool, LocalSpawner};
use futures::task::LocalSpawnExt;
use sm_ext::{
    cell_t, native, register_natives, ExecType, Executable, GameFrameHookId, HandleId, HandleRef,
    HandleType, HasHandleType, IExtension, IExtensionInterface, IForwardManager, IHandleSys,
    IPluginContext, IPluginFunction, IShareSys, ISourceMod, ParamType, SMExtension,
};
use std::cell::RefCell;
use std::error::Error;
use std::ffi::CString;

/*
typedef UdpSocketBoundCallback = function void(UdpSocket socket, any data);
typedef UdpSocketConnectedCallback = function void(UdpSocket socket, any data);
typedef UdpSocketReceiveCallback = function void(UdpSocket socket, const char[] buffer, int length, any data);

methodmap UdpSocket < Handle {
    public static native void Bind(UdpSocketBoundCallback callback, const char[] ip = "0.0.0.0", int port = 0, any data = 0);
    public native void Connect(UdpSocketConnectedCallback callback, const char[] host, int port, any data = 0);
    public native void Send(const char[] data, int length = 0);
    public native void Receive(UdpSocketReceiveCallback callback, int length = 4096, any data = 0);
}

public void OnSocketReceive(UdpSocket socket, const char[] buffer, int length, any data)
{
    PrintToServer(">>> \"%s\"", buffer);

    socket.Receive(OnSocketReceive, _, data);
}

public void OnSocketConnected(UdpSocket socket, any data)
{
    socket.Receive(OnSocketReceive);

    socket.Send("Hello, World!");
}

public void OnSocketBound(UdpSocket socket, any data)
{
    socket.Connect(OnSocketConnected, "echo.u-blox.com", 7);
}

public void OnPluginStart()
{
    UdpSocket.Bind(OnSocketBound);
}
*/

#[derive(Debug)]
struct UdpSocketContext {
    socket: Option<UdpSocket>,
}

impl HasHandleType for UdpSocketContext {
    fn handle_type<'ty>() -> &'ty HandleType<Self> {
        MyExtension::udp_handle_type()
    }
}

#[native]
fn native_udpsocket_bind(
    ctx: &IPluginContext,
    mut callback: IPluginFunction,
    host: &str,
    port: i32,
    data: cell_t,
) -> Result<(), Box<dyn Error>> {
    let context = UdpSocketContext { socket: None };

    let mut context = context.into_handle()?;
    let handle = context.clone_handle(ctx.get_identity())?;

    let mut forward = MyExtension::forwardsys().create_private_forward(
        None,
        ExecType::Single,
        &[ParamType::Cell, ParamType::Cell],
    )?;

    forward.add_function(&mut callback);

    let host = host.to_string();

    MyExtension::spawner().spawn_local(async move {
        let future = async {
            let socket = UdpSocket::bind((host.as_str(), port as u16)).await?;

            context.socket = Some(socket);

            forward.push(handle)?;
            forward.push(data)?;
            forward.execute()?;

            Ok(())
        };

        if let Result::<(), Box<dyn Error>>::Err(_) = future.await {
            let future = async {
                // We have to manually release the handle we cloned for the plugin.
                // TODO: This should be using the plugins identity, but we don't have access to it here safely.
                // TODO: Might need to adjust the type permissions to allow the ident Delete access.
                MyExtension::udp_handle_type()
                    .free_handle(handle, MyExtension::myself().get_identity())?;

                // TODO: This should be calling a special error callback and providing the error details.
                forward.push(HandleId::invalid())?;
                forward.push(data)?;
                forward.execute()?;

                Ok(())
            };

            if let Result::<(), Box<dyn Error>>::Err(err) = future.await {
                MyExtension::log_error(format!(
                    "error occurred when calling async error callback: {}",
                    err
                ));
            }
        }
    })?;

    Ok(())
}

#[native]
fn native_udpsocket_connect(
    ctx: &IPluginContext,
    handle: HandleId,
    mut callback: IPluginFunction,
    host: &str,
    port: i32,
    data: cell_t,
) -> Result<(), Box<dyn Error>> {
    let context = HandleRef::new(MyExtension::udp_handle_type(), handle, ctx.get_identity())?;

    let mut forward = MyExtension::forwardsys().create_private_forward(
        None,
        ExecType::Single,
        &[ParamType::Cell, ParamType::Cell],
    )?;

    forward.add_function(&mut callback);

    let host = host.to_string();

    MyExtension::spawner().spawn_local(async move {
        let future = async {
            context
                .socket
                .as_ref()
                .unwrap()
                .connect((host.as_str(), port as u16))
                .await?;

            // TODO: The plugin could've free'd this handle before we're passing it back.
            // We'll be safe because our HandleRef would stop it actually being free'd before
            // we're done, but we don't want to be passing invalid handles to callbacks.
            // The answer is probably to try reading it again after the await (which would
            // require using the ident identity), but I'm not sure how much SM is guaranteed
            // to return an error with handle reuse. Really it looks like we need a concept of
            // a "weak" handle clone which can ask SM if there are any "strong" refs remaining.
            // (But probably best not to actually free the object until the weak ones are closed as well?)
            // See also: The error handling path in native_udpsocket_bind.
            forward.push(handle)?;
            forward.push(data)?;
            forward.execute()?;

            Ok(())
        };

        if let Result::<(), Box<dyn Error>>::Err(_) = future.await {
            let future = async {
                // TODO: This should be calling a special error callback and providing the error details.
                forward.push(HandleId::invalid())?;
                forward.push(data)?;
                forward.execute()?;

                Ok(())
            };

            if let Result::<(), Box<dyn Error>>::Err(err) = future.await {
                MyExtension::log_error(format!(
                    "error occurred when calling async error callback: {}",
                    err
                ));
            }
        }
    })?;

    Ok(())
}

#[native]
fn native_udpsocket_send(
    ctx: &IPluginContext,
    handle: HandleId,
    data: &str,
    _length: i32,
) -> Result<(), Box<dyn Error>> {
    let context = HandleRef::new(MyExtension::udp_handle_type(), handle, ctx.get_identity())?;

    // TODO: This needs to be a &[u8], not a string.
    let data = data.to_string();

    MyExtension::spawner().spawn_local(async move {
        let future = async {
            context
                .socket
                .as_ref()
                .unwrap()
                .send(data.as_bytes())
                .await?;

            Ok(())
        };

        // TODO: Error callback.
        if let Result::<(), Box<dyn Error>>::Err(err) = future.await {
            MyExtension::log_error(format!("error occurred when sending data: {}", err));
        }
    })?;

    Ok(())
}

#[native]
fn native_udpsocket_receive(
    ctx: &IPluginContext,
    handle: HandleId,
    mut callback: IPluginFunction,
    length: i32,
    data: cell_t,
) -> Result<(), Box<dyn Error>> {
    let context = HandleRef::new(MyExtension::udp_handle_type(), handle, ctx.get_identity())?;

    let mut forward = MyExtension::forwardsys().create_private_forward(
        None,
        ExecType::Single,
        &[
            ParamType::Cell,
            ParamType::String,
            ParamType::Cell,
            ParamType::Cell,
        ],
    )?;

    forward.add_function(&mut callback);

    // TODO: This is a super worse version of the case in native_udpsocket_connect, as we could
    // be waiting a very long time for the next data to arrive, so we need to somehow cancel this
    // when the plugin is done with the socket (or gets unloaded!)
    MyExtension::spawner().spawn_local(async move {
        let future = async {
            let mut buffer = vec![0; length as usize];

            let length = context
                .socket
                .as_ref()
                .unwrap()
                .recv(buffer.as_mut_slice())
                .await?;

            // TODO: This is all hackery.
            // let buffer = CString::new(buffer)?;
            let buffer = unsafe { CString::from_vec_unchecked(buffer) };

            forward.push(handle)?;
            forward.push(buffer.as_c_str())?;
            forward.push(length as i32)?;
            forward.push(data)?;
            forward.execute()?;

            Ok(())
        };

        // TODO: Error callback.
        if let Result::<(), Box<dyn Error>>::Err(err) = future.await {
            MyExtension::log_error(format!("error occurred when receiving data: {}", err));
        }
    })?;

    Ok(())
}

extern "C" fn on_game_frame(_: bool) {
    let _ = std::panic::catch_unwind(|| {
        let mut pool = MyExtension::get().pool.borrow_mut();

        // TODO: This could do a lot of work, might want one of the alternative options instead.
        pool.run_until_stalled();
    });
}

#[derive(Default, SMExtension)]
#[extension(name = "Sprocket")]
pub struct MyExtension {
    myself: Option<IExtension>,
    forwardsys: Option<IForwardManager>,
    smutils: Option<ISourceMod>,
    game_frame_hook: Option<GameFrameHookId>,
    udp_handle_type: Option<HandleType<UdpSocketContext>>,
    pool: RefCell<LocalPool>,
    spawner: Option<LocalSpawner>,
}

impl MyExtension {
    fn get() -> &'static Self {
        EXTENSION_GLOBAL.with(|ext| unsafe { &(*ext.borrow().unwrap()).delegate })
    }

    fn myself() -> &'static IExtension {
        Self::get().myself.as_ref().unwrap()
    }

    fn forwardsys() -> &'static IForwardManager {
        Self::get().forwardsys.as_ref().unwrap()
    }

    fn log_error(msg: String) {
        Self::get()
            .smutils
            .as_ref()
            .unwrap()
            .log_error(Self::myself(), msg);
    }

    fn udp_handle_type() -> &'static HandleType<UdpSocketContext> {
        Self::get().udp_handle_type.as_ref().unwrap()
    }

    fn spawner() -> LocalSpawner {
        Self::get().spawner.clone().unwrap()
    }
}

impl IExtensionInterface for MyExtension {
    fn on_extension_load(
        &mut self,
        myself: IExtension,
        sys: IShareSys,
        _late: bool,
    ) -> Result<(), Box<dyn Error>> {
        // This would be better in a constructor so we don't need the Option<> wrapper.
        self.spawner = Some(self.pool.borrow().spawner());

        let handlesys: IHandleSys = sys.request_interface(&myself)?;
        let forwardsys: IForwardManager = sys.request_interface(&myself)?;
        let smutils: ISourceMod = sys.request_interface(&myself)?;

        self.udp_handle_type = Some(handlesys.create_type("UdpSocket", myself.get_identity())?);

        register_natives!(
            &sys,
            &myself,
            [
                ("UdpSocket.Bind", native_udpsocket_bind),       //
                ("UdpSocket.Connect", native_udpsocket_connect), //
                ("UdpSocket.Send", native_udpsocket_send),       //
                ("UdpSocket.Receive", native_udpsocket_receive), //
            ]
        );

        self.game_frame_hook = Some(smutils.add_game_frame_hook(on_game_frame));

        self.myself = Some(myself);
        self.forwardsys = Some(forwardsys);
        self.smutils = Some(smutils);

        Ok(())
    }

    fn on_extension_unload(&mut self) {
        // Finish off everything in the async queue before unloading.
        // TODO: Might want a timeout here?
        self.pool.borrow_mut().run();

        self.game_frame_hook = None;
        self.udp_handle_type = None;
    }
}
