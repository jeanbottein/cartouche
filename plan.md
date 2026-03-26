can you propose a better flow. I was thinking
data structure : 
inside each game folder (1 level under the free game path), a .gamer-sidekick folder containing : 
game.json (same data as current manifest launch json file) and the 4 images for steam (cover, icon , ...)

flow : 
1-parse games into a memory database (looking for existing .gamer-sidekick folder containing manifest.json, and images)
2-calculate missing data(logic in manifester.py)
3-fetch data from steamgridb if we have the api key
4-if persistance is allowed, persist data into subfolders .gamersidekick + images 
5-if export to steam is allowed, remove shortcutd made previoulsy
6-if export to steam is allowed, insert/update shortcuts to steam
7-if export to steam ROM manager is enabled, create a manifests.json (by default in the free game folder, but customizable) 
7-if patch is enabled, execute patcher.py
8-if backup is enabled, exectuer saver.py
9-run post commands if any

it's the time to rethink the architecture, make a big plan, rename the files, the methods, big refactor
