var app = WebApplication.CreateBuilder(args).Build();

app.MapGet("/", () => "Harbormap end-to-end fixture: .NET");

app.Run();
