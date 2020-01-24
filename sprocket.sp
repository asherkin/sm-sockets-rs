#pragma semicolon 1
#pragma newdecls required

#include <sprocket>

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
