## About

Super simple tool for parsing **swagger 2.0** and **openApi 3.0**  schemas into **.proto** files
The thing was born in the process of development of another project, but will be supported (maybe). 
Parser is not customizable yet
Below I will write todo-list (maybe)

To run it you can just make a 

    cargo run

From a box, it will take a file with the name swagger.json (*included in repo, but you can put your own*) and return a file with the name api.proto

*fn main is created just for the sake of example*


upd: 
also while refactoring i'm adding a .proto to ProtoFile model parser