## App
app-title = Tina

## Página inicial
init-page-description = Inicializando…

## Página de erro
error-page-title = Algo deu errado

## Página de sincronização
sync-page-title = Sincronizando mensagens
sync-stage-initial = Baixando seu histórico de mensagens…
sync-stage-status-v3 = Sincronizando atualizações de status…
sync-stage-recent = Baixando mensagens recentes…
sync-stage-full = Baixando histórico completo…
sync-stage-push-name = Sincronizando contatos…
sync-stage-non-blocking = Sincronizando extras…
sync-stage-on-demand = Baixando histórico solicitado…
sync-stage-other = Sincronizando ({ $type })…
sync-reconnect-description =
    { $count ->
        [0] Recebendo mensagens…
        [one] { $count } mensagem recebida
       *[other] { $count } mensagens recebidas
    }
sync-skip = Pular

## Página de reparo
repair-title-starting = Iniciando…
repair-title-repairing = Reparando…
repair-description-starting = Lendo seus dados do WhatsApp.

## Avisos / notificações
toast-webp-not-found = Aviso: Suporte a WebP não encontrado! Figurinhas podem não carregar. Instale webp-pixbuf-loader.
toast-disconnected = Desconectado: { $reason }
toast-download-failed = Falha no download
retry = Tentar novamente

## Página de login
login-link-phone = Vincule com seu telefone
login-scan-qr = Escaneie o QR com o WhatsApp no seu telefone.
login-step-1 = 1.  Abra o WhatsApp no seu telefone
login-step-2 = 2.  Toque em Menu ou Configurações e escolha Aparelhos conectados
login-step-3 = 3.  Aponte seu telefone para a tela

## Menu de perfil
profile-tooltip = Perfil
profile-not-connected = Não conectado
preferences = Preferências
log-out = Sair

## Diálogo de configurações
settings-title = Preferências
settings-general = Geral
settings-downloads = Downloads
settings-downloads-description = Quando baixar anexos de imagem, vídeo e áudio.
settings-download-method = Método de download
settings-download-method-subtitle = Sob demanda: baixado assim que a mensagem fica visível. Manual: somente ao tocar no placeholder.
settings-download-on-demand = Sob demanda
settings-download-manual = Manual
settings-download-eager = Antecipado
settings-storage = Armazenamento
settings-disk-usage = Uso de disco
settings-disk-total = Total: { $size }
settings-database = Banco de dados
settings-database-subtitle = Mensagens, chats, contatos (tina.db).
settings-media = Mídia
settings-media-subtitle = Imagens, vídeos, áudios, documentos.
settings-clear-media = Limpar cache de mídia
settings-avatars = Avatares
settings-avatars-subtitle = Fotos de perfil em cache no disco.
settings-clear-avatars = Limpar cache de avatares
settings-maintenance = Manutenção
settings-repair = Reparar
settings-repair-subtitle = Re-baixar contatos, grupos e metadados de canais do WhatsApp sem reconectar.
settings-repair-run = Executar
settings-about = Sobre
settings-version = Versão
settings-memory-group = Memória
settings-memory-gtk = Interface
settings-memory-nanachi = Serviço de mensagens
settings-memory-nanachi-stopped = Parado

## Barra lateral
sidebar-filter-all = Todos
sidebar-filter-groups = Grupos
sidebar-filter-channels = Canais
sidebar-filter-status = Status
sidebar-search = Pesquisar
sidebar-no-status = Nenhuma atualização de status
sidebar-no-status-description = Atualizações de status recentes dos seus contatos aparecerão aqui.
sidebar-offline = Offline
sidebar-connecting = Conectando…
sidebar-catching-up = Atualizando
sidebar-pulling-history = Baixando histórico
sidebar-syncing = Sincronizando

## Painel de chat
pane-toggle-sidebar = Alternar barra lateral
pane-move-right = Mover aba para a divisão direita
pane-move-left = Mover aba para a divisão esquerda

## Menu de contexto da linha de chat
context-open = Abrir
context-open-new-tab = Abrir em nova aba
context-pin = Fixar
context-unpin = Desafixar

## Preview da linha de chat
preview-photo = 📷 Foto
preview-voice-note = 🎤 Mensagem de voz
preview-voice-duration = 🎤 { $min }:{ $sec }
preview-video = 🎬 Vídeo
preview-video-duration = 🎬 Vídeo { $min }:{ $sec }
preview-sticker = 🎴 Figurinha
preview-document = 📄 Documento
preview-contact = 👤 Contato
preview-location = 📍 Localização
preview-live-location = 📍 Localização em tempo real
preview-you = Você: { $text }
preview-sender = { $short }: { $text }

## Compositor de mensagens
compose-attach = Anexar
compose-stickers = Figurinhas
compose-message-placeholder = Mensagem…
compose-stop-recording = Parar gravação
compose-record-voice = Gravar áudio
compose-send = Enviar
compose-photo = Foto
compose-video = Vídeo
compose-audio-file = Arquivo de áudio
compose-sticker-file = Figurinha (arquivo)
compose-document = Documento

## Rótulos somente leitura
readonly-newsletter = Você não pode responder a canais.
readonly-status = Atualizações de status não podem ser respondidas aqui.
readonly-broadcast = Listas de transmissão são somente leitura.
readonly-default = Somente leitura.

## Diálogos de arquivo
file-dialog-send-photo = Enviar uma foto
file-dialog-send-video = Enviar um vídeo
file-dialog-send-audio = Enviar áudio
file-dialog-send-voice = Enviar um áudio de voz
file-dialog-send-sticker = Enviar uma figurinha
file-dialog-send-document = Enviar um documento
file-filter-images = Imagens
file-filter-videos = Vídeos
file-filter-audio = Áudio
file-filter-stickers = Figurinhas (.webp)
file-filter-all = Todos os arquivos

## Diálogo de pré-visualização de envio
send-cancel = Cancelar
send-send = Enviar
send-caption-placeholder = Adicionar legenda…
send-heading-photo = Enviar foto
send-heading-video = Enviar vídeo
send-heading-audio = Enviar áudio
send-heading-voice = Enviar áudio de voz
send-heading-sticker = Enviar figurinha
send-heading-document = Enviar documento

## Balão de mensagem
sender-you = Você
sender-unknown = Desconhecido
quoted-replied-message = Mensagem citada
media-image = Imagem
media-voice-audio = Voz / Áudio
media-video = Vídeo
media-sticker = Figurinha
media-document = Documento
media-attachment = Anexo

## Ações na linha de mensagem
download-retry = Tentar novamente
download-download = Baixar
play = Reproduzir
open-externally = Abrir externamente

## Lightbox
lightbox-video = Vídeo
lightbox-sticker = Figurinha
lightbox-image = Imagem
lightbox-open-externally = Abrir externamente
lightbox-save-as = Salvar como…
lightbox-save-media = Salvar mídia

## Visualizador de stories
stories-status-subtitle = Status
stories-photo-downloading = Baixando foto…
stories-video-downloading = Baixando vídeo…
stories-status-update = Atualização de status

## Divisor de dia
day-today = Hoje
day-yesterday = Ontem

## Preferência de idioma
settings-language-group = Idioma
settings-language = Idioma do app
settings-language-subtitle = Aplicado após reiniciar.
settings-language-system = Padrão do sistema
settings-language-en = English
settings-language-pt-br = Português (Brasil)
toast-language-changed = Idioma alterado. Reinicie o Tina para aplicar.
