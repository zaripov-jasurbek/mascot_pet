Mascot - voice desktop assistant
=================================

HOW TO START
  1. Unzip this folder completely. Do not run mascot.exe from inside the zip.
  2. Double-click mascot.exe.
  3. Wait a few seconds until the character appears on screen.

HOW TO USE VOICE
  Speak into the microphone, starting with "assistant":

    "assistant open telegram"             - starts an application
    "assistant open youtube in browser"   - opens a website
    "assistant open github in browser"    - opens a site through search or history

  Commands without the word "assistant" are ignored to avoid accidental triggers.

OPTIONAL SETTINGS
  You can edit voice.toml with Notepad:
    wake   - wake word, default "assistant"
    [site] - your own quick website shortcuts

  Restart mascot.exe after changing voice.toml.

IF IT DOES NOT START
  * Windows 10/11 and a Vulkan-capable GPU are required.
  * Windows may warn about an unknown publisher: click "More info" -> "Run anyway".
  * If the microphone does not work, check Windows sound settings and microphone permissions.
