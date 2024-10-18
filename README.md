[TechieW-Loader-Link]: https://www.nexusmods.com/eldenring/mods/117  
[Download-Link]: https://github.com/WardLordRuby/elden_mod_loader_gui/releases/download/v0.9.7-beta/elden_mod_loader_gui.exe  
[Nexus-Link]: https://www.nexusmods.com/eldenring/mods/4825
<div align="center">
    <img src="https://raw.githubusercontent.com/WardLordRuby/elden_mod_loader_gui/main/ui/assets/EML-icon.png" width="20%" height="20%">
</div>

# Elden Mod Loader GUI   
[![GitHub Downloads](https://img.shields.io/github/downloads/WardLordRuby/elden_mod_loader_gui/total?label=Github%20Downloads&labelColor=%2323282e&color=%230e8726)][Download-Link]
[![Nexus Downloads](https://img.shields.io/badge/dynamic/json?url=https%3A%2F%2Fgist.githubusercontent.com%2FWardLordRuby%2Fd6ef5e71d937c2310cc8058638ca17fe%2Fraw%2F&query=%24.18610093298393.mod_downloads&label=Nexus%20Downloads&labelColor=%2323282e)][Nexus-link]
[![GitHub License](https://img.shields.io/github/license/WardLordRuby/elden_mod_loader_gui?label=License&labelColor=%2323282e)](LICENSE)


A simple Mod Manager for Elden Mod Loader  
This is a GUI tool that wraps [Elden Mod Loader][TechieW-Loader-Link] by: TechieW  
His loader files are required to be installed for this app to work  

This app also does not disable Easy Anti-cheat  
Make sure to have EAC disabled before launching Elden Ring with mods installed  

## Compatibility
Supported on Windows 10 and later.

## Installation  

1. Download [Elden Mod Loader][TechieW-Loader-Link] and extract files to "[your_game_directory]/ELDEN RING/Game/"
2. Download [elden_mod_loader_gui.exe][Download-Link] and run from anywhere  
3. Install mods  
   * As you normally would
   * Using *elden_mod_loader_gui* to import files into your /mods/ directory  

##### Notes:  

The app will generate its own config file and attempt to locate the install directory of Elden Ring. If it succeeds and finds that *Elden Mod Loader* is
installed, the app is immediately ready to use! Otherwise it will prompt you to select the install directory for your copy of Elden Ring. If you move the
app you will have to move the config file as well. It is not recommended to edit this apps ini file manually. If you want to disable logging you can set
'save_log' to 'false' in "EML_gui_config.ini"  

## Features  

* Set the load order of each mod  
* Edit a text file by selecting it from the list of registered files  
* Install mods inside the app  
* Scan /mods/ for mods already installed to auto import  
* Option to uninstall after de-registering a mod from within the app  
* Easily open game directory with windows explorer  
* Edit load_delay time and show_terminal in settings 
* App will attempt to locate game dir for you  
* On game dir verified you can Register mods to the app  
* Easily add more files to a registered mod. eg. config files  
* Easily toggle each registered mod on and off  
* View registed mods details, State, Files, User provided Name  
* If registred mod contains an ini file you are able to open config files from within the app  
* Swap between Dark and Light themes for the app  
* On de-registration of a mod app will make sure it is set to enabled  
* Writes logs to file "EML_gui_log.txt"  
* Logs panic messages to file

## Screenshots  

<div id="image-screenshots">
    <img src="https://i.imgur.com/qJC5Tyy.png" width="26%" height="26%">
    <img src="https://i.imgur.com/vuMAqmt.png" width="26%" height="26%">
    <img src="https://i.imgur.com/xd0XlBC.png" width="26%" height="26%">
    <img src="https://i.imgur.com/xRe7Ig4.png" width="26%" height="26%">
</div>
