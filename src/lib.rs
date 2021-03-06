use std::cell::RefCell;
use std::error::Error;
use std::ffi::CString;
use std::rc::Rc;
use std::time::Duration;

use async_std::net::UdpSocket;
use futures::executor::{LocalPool, LocalSpawner};
use futures::task::LocalSpawnExt;

use sm_ext::{
    cell_t, native, register_natives, ExecType, Executable, GameFrameHookId, HandleAccess,
    HandleAccessRestriction, HandleId, HandleType, IExtension, IExtensionInterface,
    IForwardManager, IHandleSys, IPluginContext, IPluginFunction, IShareSys, ISourceMod, ParamType,
    SMExtension,
};

const HANDLE_LIVENESS_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug)]
struct UdpSocketContext {
    socket: Option<UdpSocket>,
}

#[native]
#[allow(clippy::redundant_pattern_matching)]
fn native_udpsocket_bind(
    ctx: &IPluginContext,
    mut callback: IPluginFunction,
    host: &str,
    port: i32,
    data: cell_t,
) -> Result<(), Box<dyn Error>> {
    let context = Rc::new(RefCell::new(UdpSocketContext { socket: None }));
    let handle =
        MyExtension::udp_handle_type().create_handle(context.clone(), ctx.get_identity(), None)?;

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

            {
                let mut context = context.try_borrow_mut()?;
                context.socket = Some(socket);
            }

            forward.push(handle)?;
            forward.push(data)?;
            forward.execute()?;

            Ok(())
        };

        if let Result::<(), Box<dyn Error>>::Err(_) = future.await {
            let future = async {
                // We have to manually release the handle we created for the plugin.
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
#[allow(clippy::redundant_pattern_matching)]
fn native_udpsocket_connect(
    ctx: &IPluginContext,
    handle: HandleId,
    mut callback: IPluginFunction,
    host: &str,
    port: i32,
    data: cell_t,
) -> Result<(), Box<dyn Error>> {
    let context = MyExtension::udp_handle_type().read_handle(handle, ctx.get_identity())?;

    let mut forward = MyExtension::forwardsys().create_private_forward(
        None,
        ExecType::Single,
        &[ParamType::Cell, ParamType::Cell],
    )?;

    forward.add_function(&mut callback);

    let host = host.to_string();

    MyExtension::spawner().spawn_local(async move {
        let future = async {
            loop {
                let result = async_std::future::timeout(
                    HANDLE_LIVENESS_INTERVAL,
                    context
                        .try_borrow()?
                        .socket
                        .as_ref()
                        .unwrap()
                        .connect((host.as_str(), port as u16)),
                )
                .await;

                // If we're the only reference left, the plugin's handle is gone
                // and no one cares we're connected.
                if Rc::strong_count(&context) == 1 {
                    break Ok(());
                }

                match result {
                    Ok(result) => result,
                    Err(_) => continue,
                }?;

                forward.push(handle)?;
                forward.push(data)?;
                forward.execute()?;

                break Ok(());
            }
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
    let context = MyExtension::udp_handle_type().read_handle(handle, ctx.get_identity())?;

    // TODO: This needs to be a &[u8], not a string.
    let data = data.to_string();

    MyExtension::spawner().spawn_local(async move {
        let future = async {
            context
                .try_borrow()?
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
    let context = MyExtension::udp_handle_type().read_handle(handle, ctx.get_identity())?;

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

    MyExtension::spawner().spawn_local(async move {
        let future = async {
            let mut buffer = vec![0; length as usize];

            loop {
                let result = async_std::future::timeout(
                    HANDLE_LIVENESS_INTERVAL,
                    context
                        .try_borrow()?
                        .socket
                        .as_ref()
                        .unwrap()
                        .recv(buffer.as_mut_slice()),
                )
                .await;

                // If we're the only reference left, the plugin's handle is gone
                // and no one cares about the data we've received.
                if Rc::strong_count(&context) == 1 {
                    break Ok(());
                }

                let length = match result {
                    Ok(result) => result,
                    Err(_) => continue,
                }?;

                // TODO: This is all hackery.
                // let buffer = CString::new(buffer)?;
                let buffer = unsafe { CString::from_vec_unchecked(buffer) };

                forward.push(handle)?;
                forward.push(buffer.as_c_str())?;
                forward.push(length as i32)?;
                forward.push(data)?;
                forward.execute()?;

                break Ok(());
            }
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
    udp_handle_type: Option<HandleType<RefCell<UdpSocketContext>>>,
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

    fn udp_handle_type() -> &'static HandleType<RefCell<UdpSocketContext>> {
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

        let mut access = HandleAccess::new();
        access.delete_access = HandleAccessRestriction::OwnerAndIdentity;

        self.udp_handle_type =
            Some(handlesys.create_type("UdpSocket", Some(&access), myself.get_identity())?);

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
