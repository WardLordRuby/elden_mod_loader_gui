# elden_mod_loader_gui

A simple GUI for Elden Mod Loader

## About

Work in progress

### To-do:

* Sanity check front end in case of throwing an error

* Next Features:  
*   Have mods sorted by Load order, able to adjust load order in app

### feats:

* App Icon  
* Feat: Edit load_delay time and show_terminal in settings 
* Feat: App will attempt to locate game dir for you  
* Feat: On game dir verified you can Register mods to the app  
* Feat: Easily add more files to a registered mod. eg. config files  
* Feat: Easily toggle each registered mod on and off  
* Feat: View registed mods details, State, Files, User provided Name  
* Feat: If registred mod contains an ini file you are able to open config files from within the app  
* Feat: Swap between Dark and Light themes for the app  
* Feat: On de-registration of a mod app will make sure it is set to enabled  
* Design: Main UI, Settings subpage, and subpage to edit registered mods  
* Ability: deserialize backend rust modData to data slint can use  
* Ability: display error message to user in dialog  
* Ability: parse Array of Paths, Paths and bools from ini  
* Abiltiy: only read valid Arrays, Paths, and bools  
* Ability: check if directory contains files  
* Ability: try locate a dir on user drive  
* Ability: Togglefiles to on and off state  
* Ability: Error check config file and return all usable data  