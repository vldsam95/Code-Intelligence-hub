using Demo.Services;

namespace Demo.App
{
    public sealed class Program
    {
        public static void Main()
        {
            var service = new GreetingService();
            service.Greet("Code Intelligence");
        }
    }
}

namespace Demo.Services
{
    public sealed class GreetingService
    {
        public string Greet(string name) => $"Hello {name}";
    }
}
