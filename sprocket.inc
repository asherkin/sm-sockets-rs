#if defined _sprocket_included_
    #endinput
#endif
#define _sprocket_included_

typedef UdpSocketBoundCallback = function void(UdpSocket socket, any data);
typedef UdpSocketConnectedCallback = function void(UdpSocket socket, any data);
typedef UdpSocketReceiveCallback = function void(UdpSocket socket, const char[] buffer, int length, any data);

methodmap UdpSocket < Handle {
    public static native void Bind(UdpSocketBoundCallback callback, const char[] ip = "0.0.0.0", int port = 0, any data = 0);
    public native void Connect(UdpSocketConnectedCallback callback, const char[] host, int port, any data = 0);
    public native void Send(const char[] data, int length = 0);
    public native void Receive(UdpSocketReceiveCallback callback, int length = 4096, any data = 0);
}

public Extension __ext_sprocket = {
    name = "Sprocket",
    file = "sprocket.ext",
#if defined AUTOLOAD_EXTENSIONS
    autoload = 1,
#else
    autoload = 0,
#endif
#if defined REQUIRE_EXTENSIONS
    required = 1,
#else
    required = 0,
#endif
};
