# Release do Arcadia (canal empacotado)

O updater do app empacotado (AppImage/NSIS) lê o `latest*.yml` anexado ao
release. Anexar o yml errado (ou esquecer) faz o app baixar a versão errada —
siga a ordem.

## Automated release path

Push a tag whose version matches `app/package.json`:

```bash
git tag v1.5.6
git push origin v1.5.6
```

GitHub Actions runs the full test, typecheck and front-end build first. Then it
builds the NSIS Windows installer and Linux AppImage and uploads both to a
draft release. Once both jobs succeed, verify the assets and publish the draft.
`electron-updater` reads `latest.yml` on Windows and `latest-linux.yml` on Linux.

Do not publish a tag before the version in `app/package.json` is updated. The
workflow rejects mismatched tags.

Portable and zip packages remain manual downloads. They cannot replace their
own executable while running, so they show the release page instead of
pretending to auto-update.

## Manual checklist

1. **Versão** — confira `"version"` em `app/package.json` (ex.: `1.5.0`).
2. **Limpe os yml antigos** — `rm -f app/release/latest*.yml`. O `release/`
   passa a acumular yml a partir do primeiro build com `publish`; reaproveitar
   um yml velho faz o updater oferecer uma versão que não é a publicada.
3. **Builds da MESMA versão** (os 4 assets que o passo 5 publica saem daqui):

   ```bash
   cd app
   npm run dist:appimage   # gera release/latest-linux.yml
   npm run dist:nsis       # gera release/latest.yml
   npm run dist:portable   # gera Arcadia-<versao>-x64.exe
   npm run dist:zip        # gera Arcadia-<versao>-x64.zip
   ```

4. **Confira os yml e os blockmaps gerados**:

   ```bash
   grep -H '^version:' release/latest.yml release/latest-linux.yml
   ls release/*.blockmap
   ```

   As duas linhas têm que mostrar a versão do passo 1. Os `.blockmap` são o que
   habilita o download diferencial — sem eles o updater baixa o arquivo inteiro.
   No Linux o AppImage **embute** o blockmap (não gera `.blockmap` separado);
   no Windows o arquivo é `Arcadia-Setup-<versao>-x64.exe.blockmap`.
5. **Publique os 4 assets + os 2 yml + os 2 blockmaps** (não-draft e
   não-prerelease: o electron-updater ignora draft/prerelease):

   ```bash
   gh release create v1.5.0 --title "Arcadia 1.5.0" \
     release/Arcadia-Setup-1.5.0-x64.exe \
     release/Arcadia-1.5.0-x64.exe \
     release/Arcadia-1.5.0-x64.zip \
     release/Arcadia-1.5.0-x86_64.AppImage \
     release/latest.yml release/latest-linux.yml \
     release/Arcadia-Setup-1.5.0-x64.exe.blockmap
   ```

6. **Valide o release publicado**:

   ```bash
   gh release view v1.5.0 --json assets --jq '.assets[].name' | sort
   ```

   Esperado: `Arcadia-1.5.0-x64.exe`, `Arcadia-1.5.0-x64.zip`,
   `Arcadia-1.5.0-x86_64.AppImage`, `Arcadia-1.5.0-x86_64.AppImage.blockmap`,
   `Arcadia-Setup-1.5.0-x64.exe`, `Arcadia-Setup-1.5.0-x64.exe.blockmap`,
   `latest-linux.yml`, `latest.yml`.

   > O `v1.4.1` foi publicado **antes** deste recurso e não tem yml nem
   > blockmap — validar por ele não serve. A validação é sempre do release que
   > acabou de ser publicado.

## Limitações aceitas (documentadas)

- **Windows sem assinatura de código**: o updater funciona, mas
  SmartScreen/Defender avisam tanto no setup quanto no update. Exige
  certificado pago — fora de escopo por agora.
- **NSIS por-máquina** (`oneClick: false`, `allowToChangeInstallationDirectory:
  true`): o update dispara UAC. Se o usuário negar, a instalação não acontece e
  o aviso "Pronto — Reiniciar agora" volta no próximo boot.
- **Portable e zip** não têm auto-update: o app mostra "Esta versão não se
  atualiza sozinha" com link para a página de releases.
- **AppImage em pasta sem escrita** (`/opt`, montagem read-only): o
  `quitAndInstall` falha e o app mostra o aviso com link.
- **Toggle `check_updates_on_start` desligado**: não há checagem de boot —
  inclusive o reaviso de "pronto" de um download pendente (D6). O caminho é
  "Procurar atualizações" em Configurações ou religar o toggle.
- **Download parcial não retoma byte a byte** (electron-updater 6 descarta o
  temporário): o arquivo **completo** em cache é reaproveitado no próximo boot;
  com os `.blockmap` anexados, NSIS e AppImage usam download diferencial.
