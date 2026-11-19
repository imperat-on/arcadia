#!/usr/bin/env python3
"""Servidor Phoenix-lite de mentira, só para provar o cliente Rust.

Aceita o upgrade WebSocket, lê o phx_join, responde phx_reply ok, empurra dois
eventos postgres_changes e fecha. É a sequência exata do protocolo descrito em
app/electron/supabase/client.js.
"""
import base64
import hashlib
import json
import socket
import struct
import sys
import time

GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"


def ler_quadro(conn):
    """Lê um frame WebSocket do cliente (mascarado)."""
    cabecalho = conn.recv(2)
    if len(cabecalho) < 2:
        return None, None
    opcode = cabecalho[0] & 0x0F
    mascarado = cabecalho[1] & 0x80
    tamanho = cabecalho[1] & 0x7F
    if tamanho == 126:
        tamanho = struct.unpack(">H", conn.recv(2))[0]
    elif tamanho == 127:
        tamanho = struct.unpack(">Q", conn.recv(8))[0]
    mascara = conn.recv(4) if mascarado else b"\x00\x00\x00\x00"
    dados = b""
    while len(dados) < tamanho:
        pedaco = conn.recv(tamanho - len(dados))
        if not pedaco:
            break
        dados += pedaco
    if mascarado:
        dados = bytes(b ^ mascara[i % 4] for i, b in enumerate(dados))
    return opcode, dados


def escrever_texto(conn, texto):
    """Escreve um frame de texto sem máscara (servidor)."""
    dados = texto.encode()
    tamanho = len(dados)
    if tamanho < 126:
        cabecalho = struct.pack(">BB", 0x81, tamanho)
    elif tamanho < 65536:
        cabecalho = struct.pack(">BBH", 0x81, 126, tamanho)
    else:
        cabecalho = struct.pack(">BBQ", 0x81, 127, tamanho)
    conn.sendall(cabecalho + dados)


def main():
    porta = int(sys.argv[1]) if len(sys.argv) > 1 else 8765
    servidor = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    servidor.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    servidor.bind(("127.0.0.1", porta))
    servidor.listen(4)
    print(f"ouvindo em 127.0.0.1:{porta}", flush=True)

    conn, _ = servidor.accept()
    pedido = b""
    while b"\r\n\r\n" not in pedido:
        pedaco = conn.recv(4096)
        if not pedaco:
            return
        pedido += pedaco

    # Extrai a chave do handshake e responde o 101.
    chave = ""
    for linha in pedido.decode(errors="replace").split("\r\n"):
        if linha.lower().startswith("sec-websocket-key:"):
            chave = linha.split(":", 1)[1].strip()
    aceite = base64.b64encode(
        hashlib.sha1((chave + GUID).encode()).digest()
    ).decode()
    conn.sendall(
        (
            "HTTP/1.1 101 Switching Protocols\r\n"
            "Upgrade: websocket\r\n"
            "Connection: Upgrade\r\n"
            f"Sec-WebSocket-Accept: {aceite}\r\n\r\n"
        ).encode()
    )
    print("101 enviado", flush=True)

    # 1. Lê o join e ecoa o tópico/token para o log.
    opcode, dados = ler_quadro(conn)
    if dados is None:
        print("cliente fechou antes do join", flush=True)
        conn.close()
        servidor.close()
        return
    print("opcode:", opcode, flush=True)
    join = json.loads(dados)
    print("join recebido:", json.dumps(join)[:200], flush=True)

    # 2. Responde o phx_reply com status ok.
    escrever_texto(
        conn,
        json.dumps({"event": "phx_reply", "payload": {"status": "ok"}, "ref": join.get("ref")}),
    )

    # 3. Empurra eventos postgres_changes.
    for i in range(2):
        escrever_texto(
            conn,
            json.dumps(
                {
                    "event": "postgres_changes",
                    "payload": {
                        "eventType": "INSERT",
                        "table": "friendships",
                        "new": {"requester_id": f"user-{i}"},
                    },
                }
            ),
        )
        time.sleep(0.1)

    time.sleep(0.5)
    conn.close()
    servidor.close()


if __name__ == "__main__":
    main()
